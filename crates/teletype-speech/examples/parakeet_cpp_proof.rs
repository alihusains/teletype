//! Functional proof for the Parakeet.cpp backend (Fluid Speech Mini).
//!
//! Loads a GGUF model through [`ParakeetCppProvider`] and transcribes a
//! 16 kHz mono WAV file, printing the transcript. This is the end-to-end
//! proof that the mudler/parakeet.cpp engine (mixed-quant GGUF loader) works
//! in-process.
//!
//! Usage:
//!   cargo run -p teletype-speech --example parakeet_cpp_proof \
//!     -- evals/models/fluid-speech-mini-q2k-q4k.gguf evals/corpus/audio/list-01.wav

#[cfg(target_os = "macos")]
fn main() {
    use teletype_speech::SpeechProvider;

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: parakeet_cpp_proof <model.gguf> <input.wav>");
        std::process::exit(2);
    }
    let model_path = std::path::PathBuf::from(&args[0]);
    let wav_path = std::path::PathBuf::from(&args[1]);

    let (samples, rate) =
        read_wav_mono16(&wav_path).unwrap_or_else(|e| panic!("failed to read {wav_path:?}: {e}"));
    println!(
        "audio: {} samples @ {} Hz ({:.2}s)",
        samples.len(),
        rate,
        samples.len() as f64 / rate as f64
    );

    let mut provider = teletype_speech::ParakeetCppProvider::new();
    let loaded = std::time::Instant::now();
    provider
        .load(&model_path)
        .unwrap_or_else(|e| panic!("model load failed: {e}"));
    println!("model loaded in {:.2}s", loaded.elapsed().as_secs_f64());

    let started = std::time::Instant::now();
    let text = provider
        .transcribe(&samples, "en")
        .unwrap_or_else(|e| panic!("transcribe failed: {e}"));
    println!("transcribed in {:.2}s", started.elapsed().as_secs_f64());
    println!("transcript: {text}");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("parakeet.cpp backend is macOS-only");
    std::process::exit(2);
}

/// Minimal 16-bit mono PCM WAV reader (mirrors stream_spike.rs), so the
/// proof needs no decode dependency in the speech crate.
fn read_wav_mono16(path: &std::path::Path) -> Result<(Vec<f32>, u32), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut pos = 12usize;
    let (mut rate, mut channels) = (0u32, 0u16);
    let mut samples: Option<Vec<f32>> = None;
    while pos + 8 <= bytes.len() && samples.is_none() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            channels = u16::from_le_bytes(bytes[body + 2..body + 4].try_into().unwrap());
            rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().unwrap());
        } else if id == b"data" {
            let end = (body + size).min(bytes.len());
            let data = &bytes[body..end];
            let mut out = Vec::with_capacity(data.len() / 2);
            for chunk in data.chunks_exact(2) {
                out.push(i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0);
            }
            if channels != 1 {
                out.truncate(out.len() / channels as usize);
            }
            samples = Some(out);
        }
        pos = body + size + (size % 2);
    }
    let samples = samples.ok_or("no data chunk")?;
    if rate == 0 {
        return Err("no fmt chunk".into());
    }
    Ok((samples, rate))
}
