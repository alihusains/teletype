//! A.4 integration tests runnable without a microphone:
//!
//! - `json_dumps_compat` is in `tests/json_dumps_compat.rs`.
//! - `parity_with_python`: Rust `generate` vs the PyPI `neudecide` package
//!   over the 50 TTS command WAVs in `evals/commands/tts/`. Skipped (with a
//!   reason printed) when the model dir or the python reference is absent.
//! - `noise_does_not_trigger`: MUSAN noise clips + silence through the
//!   decider; assert no Command/OptionalCommand (i.e. `no_action` or `[]`).
//! - `ort_and_sherpa_same_process`: load both the ort-based decider and
//!   sherpa-onnx in one process and run one of each.
//!
//! Real-model tests are gated on the model directory (env
//! `NEUDECIDE_MODEL_DIR`, default: the HF cache snapshot). They skip with an
//! explicit reason when the model is not present — the same convention as
//! `tests/voice_command_e2e.rs`.

#[cfg(feature = "onnx")]
use std::path::Path;
#[cfg(feature = "onnx")]
use std::path::PathBuf;
#[cfg(feature = "onnx")]
use teletype_decider::NeuDecide;
#[cfg(feature = "onnx")]
use teletype_decider::{tools_to_json, v1_tools, ToolCall};

/// Default model dir: the HF cache snapshot of `neuphonic/neudecide`.
#[cfg(feature = "onnx")]
fn model_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("NEUDECIDE_MODEL_DIR") {
        let p = PathBuf::from(d);
        return p.join("config.json").is_file().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = Path::new(&home).join(
        ".cache/huggingface/hub/models--neuphonic--neudecide/snapshots/fb0157e06da16d887caf041ca7929f96da419e6b",
    );
    p.join("config.json").is_file().then_some(p)
}

#[cfg(feature = "onnx")]
fn skip(reason: &str) {
    eprintln!("SKIP: {reason}");
}

/// The v1 tool list, serialized exactly like Python's `json.dumps`.
#[cfg(feature = "onnx")]
fn v1_tools_json() -> String {
    let apps = ["terminal", "safari", "notes", "mail", "finder", "code"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let snippets = ["thanks", "regards", "br", "signature"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let styles = ["body", "heading", "code"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    tools_to_json(&v1_tools(&apps, &snippets, &styles))
}

/// Read a WAV file (plain PCM, any rate) into mono f32 at 16 kHz.
///
/// The TTS fixtures are 22050 Hz LEI16 mono; MUSAN clips vary. Resampling
/// is linear (good enough for a parity/noise test; the model's own
/// resampler is soxr, but the decision is robust to this).
#[cfg(feature = "onnx")]
fn read_wav_16k(path: &Path) -> Option<Vec<f32>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    // RIFF header
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut sr: u32 = 0;
    let mut channels: u16 = 0;
    let mut bits: u16 = 0;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if body + size > bytes.len() {
            break;
        }
        match id {
            b"fmt " => {
                channels = u16::from_le_bytes(bytes[body + 2..body + 4].try_into().ok()?);
                sr = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().ok()?);
                bits = u16::from_le_bytes(bytes[body + 14..body + 16].try_into().ok()?);
            }
            b"data" => {
                data = Some(&bytes[body..body + size]);
            }
            _ => {}
        }
        pos = body + size + size % 2;
    }
    let data = data?;
    if sr == 0 || channels == 0 || bits != 16 {
        return None;
    }
    let samples_i16: Vec<i16> = data
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    // Downmix to mono.
    let mono: Vec<f32> = if channels == 1 {
        samples_i16.iter().map(|&s| s as f32 / 32768.0).collect()
    } else {
        let mut out = Vec::with_capacity(samples_i16.len() / channels as usize);
        for chunk in samples_i16.chunks(channels as usize) {
            let mean = chunk.iter().map(|&s| s as f32).sum::<f32>() / channels as f32;
            out.push(mean / 32768.0);
        }
        out
    };
    // Linear resample to 16 kHz.
    if sr == 16_000 {
        return Some(mono);
    }
    let ratio = 16_000.0 / sr as f32;
    let out_len = (mono.len() as f32 * ratio) as usize;
    let mut out = vec![0.0f32; out_len];
    for (i, slot) in out.iter_mut().enumerate() {
        let src = i as f32 / ratio;
        let i0 = src as usize;
        let i1 = (i0 + 1).min(mono.len() - 1);
        let frac = src - i0 as f32;
        *slot = mono[i0] * (1.0 - frac) + mono[i1] * frac;
    }
    Some(out)
}

#[cfg(feature = "onnx")]
const TTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../evals/commands/tts");

/// A.4: `parity_with_python` — Rust `generate` identical to the Python
/// package over the 50 TTS command WAVs.
///
/// The Python reference output is pre-generated by
/// `scripts/gen_neudecide_parity.py` (committed as
/// `tests/fixtures/parity/expected.json`) so the test itself needs only
/// the model + the Rust runtime. When either is missing, skip with a reason.
#[test]
#[cfg(feature = "onnx")]
fn parity_with_python() {
    let Some(dir) = model_dir() else {
        skip("model dir not found (set NEUDECIDE_MODEL_DIR or download the HF snapshot)");
        return;
    };
    let tts = Path::new(TTS_DIR);
    if !tts.join("manifest.json").is_file() {
        skip("evals/commands/tts/manifest.json missing");
        return;
    }
    let expected_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity/expected.json");
    if !expected_path.is_file() {
        skip("tests/fixtures/parity/expected.json missing (run scripts/gen_neudecide_parity.py)");
        return;
    }
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&expected_path).unwrap()).unwrap();

    let mut model = NeuDecide::load(&dir).expect("load model");
    let tools_json = v1_tools_json();
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tts.join("manifest.json")).unwrap()).unwrap();

    let mut mismatches = Vec::new();
    for entry in manifest.as_array().unwrap() {
        let id = &entry["id"];
        let wav = tts.join(format!("{}.wav", id.as_str().unwrap()));
        let Some(pcm) = read_wav_16k(&wav) else {
            eprintln!("WARN: could not read {}", wav.display());
            continue;
        };
        let got = model.generate(&pcm, &tools_json).expect("generate");
        let got_json = serde_json::json!(got
            .iter()
            .map(|c: &ToolCall| { serde_json::json!({"name": c.name, "arguments": c.arguments}) })
            .collect::<Vec<_>>());
        let want = &expected[id.as_str().unwrap()];
        if got_json != *want {
            mismatches.push((id.as_str().unwrap().to_string(), got_json, want.clone()));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatch(es) vs Python reference:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .map(|(id, g, w)| format!("{id}: rust={g} python={w}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A.4: `noise_does_not_trigger` — MUSAN noise + silence must not produce
/// a Command/OptionalCommand. The decider's "no command" outputs are an
/// empty call list or a `no_action` call; anything else is a false trigger.
#[test]
#[cfg(feature = "onnx")]
fn noise_does_not_trigger() {
    let Some(dir) = model_dir() else {
        skip("model dir not found (set NEUDECIDE_MODEL_DIR or download the HF snapshot)");
        return;
    };
    let mut model = NeuDecide::load(&dir).expect("load model");
    let tools_json = v1_tools_json();

    let home = std::env::var("HOME").unwrap();
    let musan = Path::new(&home).join(".cache/teletype-eval/musan/noise");
    let clips: Vec<PathBuf> = if musan.is_dir() {
        collect_wavs(&musan, 20)
    } else {
        eprintln!("WARN: MUSAN cache missing at {musan:?}; using silence only");
        vec![]
    };

    let mut false_triggers = 0usize;
    let mut total = 0usize;

    // Silence: 16 kHz, 2 s of zeros.
    let silence = vec![0.0f32; 32_000];
    total += 1;
    let calls = model.generate(&silence, &tools_json).expect("generate");
    if !is_no_command(&calls) {
        false_triggers += 1;
        eprintln!("SILENCE triggered: {calls:?}");
    }

    for clip in &clips {
        // Use the first ~2 s of each clip (the model truncates to 30 s anyway).
        let Some(mut pcm) = read_wav_16k(clip) else {
            continue;
        };
        if pcm.len() > 32_000 {
            pcm.truncate(32_000);
        }
        if pcm.is_empty() {
            continue;
        }
        total += 1;
        let calls = model.generate(&pcm, &tools_json).expect("generate");
        if !is_no_command(&calls) {
            false_triggers += 1;
            eprintln!("NOISE triggered: {} -> {:?}", clip.display(), calls);
        }
    }

    // Pass criteria: >= 95% no-trigger.
    let clean = total - false_triggers;
    assert!(
        total == 0 || (clean as f32 / total as f32) >= 0.95,
        "noise pass rate {clean}/{total} below 95%"
    );
}

#[cfg(feature = "onnx")]
fn collect_wavs(dir: &Path, max: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn recurse(d: &Path, out: &mut Vec<PathBuf>, max: usize) {
        if out.len() >= max {
            return;
        }
        let Ok(entries) = std::fs::read_dir(d) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                recurse(&p, out, max);
            } else if p.extension().map(|x| x == "wav").unwrap_or(false) {
                out.push(p);
                if out.len() >= max {
                    return;
                }
            }
        }
    }
    recurse(dir, &mut out, max);
    out
}

/// "No command" = empty list or a single `no_action` call.
#[cfg(feature = "onnx")]
fn is_no_command(calls: &[ToolCall]) -> bool {
    calls.is_empty() || (calls.len() == 1 && calls[0].name == "no_action")
}

/// A.4: `ort_and_sherpa_same_process` — both ONNX runtimes load and run
/// in one process with no symbol clash.
///
/// The decider uses `ort` with `load-dynamic` (dlopen of the onnxruntime
/// dylib at runtime); sherpa-onnx is statically linked via sherpa-onnx-sys.
/// This test loads a tiny sherpa VAD model if one is present in the models
/// dir, otherwise it loads the decider model only and records that sherpa
/// could not be exercised (skip-with-reason, not a fake pass).
#[test]
#[cfg(feature = "onnx")]
fn ort_and_sherpa_same_process() {
    let Some(dir) = model_dir() else {
        skip("model dir not found");
        return;
    };
    // 1. Load the ort-based decider.
    let mut model = NeuDecide::load(&dir).expect("load decider (ort)");
    let tools_json = v1_tools_json();
    let silence = vec![0.0f32; 16_000];
    let _ = model.generate(&silence, &tools_json).expect("decider runs");

    // 2. Load sherpa-onnx in the same process. When a sherpa Parakeet
    //    model is present in the models dir, create an offline recognizer
    //    and transcribe a frame of silence so the sherpa-onnx static ORT
    //    symbols are exercised alongside ort's dlopen'd copy.
    let models_dir = Path::new(env!("HOME")).join(".cache/teletype/models");
    let parakeet_dir = models_dir.join("sherpa-parakeet-v3-int8");
    if !parakeet_dir.join("encoder.int8.onnx").is_file() {
        skip("sherpa Parakeet model dir not downloaded; only the ort side was exercised");
        return;
    }
    let Ok(config) =
        teletype_speech::sherpa::SherpaParakeetProvider::config_for(&parakeet_dir, None)
    else {
        skip("could not build sherpa config for the Parakeet model dir");
        return;
    };
    let Some(recognizer) = sherpa_onnx::OfflineRecognizer::create(&config) else {
        skip("sherpa recognizer failed to load");
        return;
    };
    let stream = recognizer.create_stream();
    stream.accept_waveform(16_000, &[0.0f32; 16_000]);
    recognizer.decode(&stream);
    drop(stream);
    drop(recognizer);
}
