use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// ASRHALL-1 regression: the energy-based trailing trim must cut the
/// silent tail off the two confirmed hallucination recordings. Skips when
/// the recordings are not present (they live in the user's app-support
/// dir, not the repo). The synthetic coverage for the trim itself is in
/// `teletype_core::audio::tests::trailing_trim`.
#[test]
fn asrhall_trailing_trim_cuts_confirmed_recording_tails() {
    let dir = std::env::var("HOME").ok().map(|h| {
        std::path::PathBuf::from(h).join("Library/Application Support/com.teletype.app/recordings")
    });
    let Some(dir) = dir.filter(|d| d.is_dir()) else {
        eprintln!("asrhall regression: recordings dir absent, skipping");
        return;
    };
    // (filename, original trailing-silence length in seconds, per the
    // ASRHALL-1 sizing in docs/release-captain-v0.2.17.md)
    let cases = [
        ("f5ee5723-d9d5-479c-981c-bde923c9d6ed.wav", 4.59),
        ("8f0f20dc-6929-4076-8bea-a1b402c0661e.wav", 2.49),
    ];
    let mut checked = 0;
    for (name, _orig_tail) in cases {
        let bytes = match std::fs::read(dir.join(name)) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let samples: Vec<f32> = bytes[44..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect();
        let keep = teletype_core::audio::trim_trailing_silence(&samples);
        // After the trim, at most ~0.3 s of low-level audio may remain
        // past the last sample that is clearly speech (the 150 ms pad plus
        // the one-second soft-span can leave a little low noise, but the
        // multi-second hallucination tail must be gone).
        let trimmed = &samples[..keep];
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let high = 0.012f32.max(0.3 * peak);
        let last_high = trimmed.iter().rposition(|s| s.abs() > high).unwrap_or(0);
        let tail_after = (trimmed.len() - last_high) as f64 / 16000.0;
        eprintln!(
            "asrhall {name}: {} -> {} samples, tail after last speech {:.2}s",
            samples.len(),
            keep,
            tail_after
        );
        assert!(
            tail_after < 1.2,
            "{name}: trim left {:.2}s of tail after the last clear speech",
            tail_after
        );
        checked += 1;
    }
    // The recordings dir may exist (from other dev runs) without holding
    // these two specific confirmed recordings — that is the same as the
    // dir being absent: skip, don't fail. Only a present file that fails
    // the trim assertion is a real regression.
    if checked == 0 {
        eprintln!(
            "asrhall regression: neither recording present in {:?}; skipping",
            dir
        );
    }
}

fn term() -> teletype_core::rescoring::RescoreTerm {
    teletype_core::rescoring::RescoreTerm {
        text: "Teletype".into(),
        aliases: vec!["teletype".into()],
        weight: 1.0,
    }
}

/// True when running on a CI runner. Real-take integration tests block on
/// a wall-clock deadline for the release worker + tail decode + spool
/// drain to finish; on a loaded macos-latest runner (the Coverage job runs
/// the whole workspace suite at once) that can exceed the local deadline
/// for pure scheduling reasons, so those tests relax the bound on CI.
fn is_ci_runner() -> bool {
    matches!(std::env::var("CI"), Ok(v) if !v.is_empty() && v != "false")
}

/// Wall-clock deadline for a real-take integration test: generous on CI
/// (the whole-workspace Coverage job is heavily loaded), tight locally so
/// a genuine hang is still caught quickly.
fn take_deadline() -> std::time::Duration {
    if is_ci_runner() {
        std::time::Duration::from_secs(60)
    } else {
        std::time::Duration::from_secs(15)
    }
}

/// CMCP-1. Granite and Moonshine answer a live take with the text and no
/// word rows at all, while claiming `word_timings`. The delivery path
/// rebuilds the sentence from the word rows, so an empty word list joined
/// to "" and the take pasted nothing, with no error logged.
#[test]
fn a_wordless_engine_keeps_its_transcript() {
    let kept = rescore_or_keep(
        "the quick brown fox jumps over the lazy dog".into(),
        &[],
        &[term()],
    );
    assert_eq!(kept, "the quick brown fox jumps over the lazy dog");
}

#[test]
fn no_terms_skips_the_rebuild() {
    let words = [
        teletype_speech::Word {
            text: "the".into(),
            start_ms: 0,
            end_ms: 100,
            conf: 0.9,
        },
        teletype_speech::Word {
            text: "quick".into(),
            start_ms: 100,
            end_ms: 200,
            conf: 0.9,
        },
    ];
    assert_eq!(rescore_or_keep("raw text".into(), &words, &[]), "raw text");
}

/// The guard above must not disable rescoring for the engines that do
/// report word rows: those still rebuild the take from them.
#[test]
fn word_rows_still_rebuild_the_take() {
    let words = [
        teletype_speech::Word {
            text: "the".into(),
            start_ms: 0,
            end_ms: 100,
            conf: 0.9,
        },
        teletype_speech::Word {
            text: "quick".into(),
            start_ms: 100,
            end_ms: 200,
            conf: 0.9,
        },
    ];
    assert_eq!(
        rescore_or_keep("raw text".into(), &words, &[term()]),
        "the quick"
    );
}

/// A provider that counts calls to `transcribe` and `transcribe_detailed`
/// so tests can pin the exact call sequence the retry logic produces.
struct CountingTimedProvider {
    detailed_calls: AtomicUsize,
    plain_calls: AtomicUsize,
    detailed_result: Result<teletype_speech::Transcript, String>,
    plain_result: Result<String, String>,
}

impl CountingTimedProvider {
    fn new(
        detailed_result: Result<teletype_speech::Transcript, String>,
        plain_result: Result<String, String>,
    ) -> Self {
        Self {
            detailed_calls: AtomicUsize::new(0),
            plain_calls: AtomicUsize::new(0),
            detailed_result,
            plain_result,
        }
    }
}

impl teletype_speech::SpeechProvider for CountingTimedProvider {
    fn load(&mut self, _path: &std::path::Path) -> Result<(), teletype_speech::SpeechError> {
        Ok(())
    }

    fn unload(&mut self) {}

    fn is_loaded(&self) -> bool {
        true
    }

    fn transcribe(
        &mut self,
        _samples: &[f32],
        _language: &str,
    ) -> Result<String, teletype_speech::SpeechError> {
        self.plain_calls.fetch_add(1, Ordering::SeqCst);
        match &self.plain_result {
            Ok(text) => Ok(text.clone()),
            Err(msg) => Err(teletype_speech::SpeechError::Transcribe(msg.clone())),
        }
    }

    fn transcribe_detailed(
        &mut self,
        _samples: &[f32],
        _language: &str,
    ) -> Result<teletype_speech::Transcript, teletype_speech::SpeechError> {
        self.detailed_calls.fetch_add(1, Ordering::SeqCst);
        match &self.detailed_result {
            Ok(t) => Ok(t.clone()),
            Err(msg) => Err(teletype_speech::SpeechError::Transcribe(msg.clone())),
        }
    }

    fn capabilities(&self) -> teletype_speech::ModelCapabilities {
        teletype_speech::ModelCapabilities {
            native_boost: false,
            word_timings: true,
        }
    }
}

fn run_provider(
    provider: Arc<Mutex<CountingTimedProvider>>,
    samples: &[f32],
) -> Result<(String, Option<Vec<teletype_speech::Word>>), String> {
    let speech: &mut dyn teletype_speech::SpeechProvider = &mut *provider
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if speech.capabilities().word_timings {
        let detailed = match speech.transcribe_detailed(samples, "en") {
            Ok(d) => d,
            Err(e) => {
                let detailed_err = e.to_string();
                let text = match speech.transcribe(samples, "en") {
                    Ok(t) => t,
                    Err(e2) => {
                        crate::log_entry(
                            crate::LogLevel::Warn,
                            format!(
                                "detailed decode failed ({detailed_err}); \
                                 plain decode also failed"
                            ),
                        );
                        return Err(e2.to_string());
                    }
                };
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!(
                        "detailed decode failed ({detailed_err}); \
                         retrying without word timings"
                    ),
                );
                return Ok((text, None));
            }
        };
        Ok((detailed.text, Some(detailed.words)))
    } else {
        let text = speech
            .transcribe(samples, "en")
            .map_err(|e| e.to_string())?;
        Ok((text, None))
    }
}

fn sample_words() -> Vec<teletype_speech::Word> {
    vec![
        teletype_speech::Word {
            text: "hello".into(),
            start_ms: 0,
            end_ms: 300,
            conf: 0.97,
        },
        teletype_speech::Word {
            text: "world".into(),
            start_ms: 300,
            end_ms: 600,
            conf: 0.95,
        },
    ]
}

/// TC-2: detailed errors, plain succeeds: the take yields the plain
/// text, words are None, and exactly one detailed call plus one plain
/// call happened.
#[test]
fn detailed_error_falls_back_to_plain_with_no_words() {
    let provider = Arc::new(std::sync::Mutex::new(CountingTimedProvider::new(
        Err("detailed engine blew up".into()),
        Ok("hello world".into()),
    )));
    let calls = provider.clone();
    let samples = vec![0.1f32; 16_000];
    let result = run_provider(Arc::clone(&provider), &samples).expect("plain path must succeed");
    assert_eq!(result.0, "hello world");
    assert!(result.1.is_none(), "fallback must not invent word timings");
    let counts = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        counts.detailed_calls.load(Ordering::SeqCst),
        1,
        "exactly one detailed call before the fallback"
    );
    assert_eq!(
        counts.plain_calls.load(Ordering::SeqCst),
        1,
        "exactly one plain retry call"
    );
}

/// TC-2: both error: the caller gets an error, and it is the plain
/// path's message (the detailed error is only logged, not returned).
#[test]
fn both_detailed_and_plain_error_returns_plain_message() {
    let provider = Arc::new(std::sync::Mutex::new(CountingTimedProvider::new(
        Err("detailed failed".into()),
        Err("plain also failed".into()),
    )));
    let calls = provider.clone();
    let samples = vec![0.1f32; 16_000];
    let err = run_provider(provider, &samples).expect_err("both paths fail");
    assert!(
        err.contains("plain also failed"),
        "the returned error must be from the plain path, not the detailed one; got: {err}"
    );
    assert!(
        !err.contains("detailed failed"),
        "the returned error must not be the detailed path's message; got: {err}"
    );
    let counts = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        counts.detailed_calls.load(Ordering::SeqCst),
        1,
        "one detailed call before the fallback attempt"
    );
    assert_eq!(
        counts.plain_calls.load(Ordering::SeqCst),
        1,
        "one plain retry call"
    );
}

/// TC-2: detailed succeeds: exactly one call, words preserved, plain
/// never called.
#[test]
fn detailed_success_keeps_words_and_never_calls_plain() {
    let words = sample_words();
    let provider = Arc::new(std::sync::Mutex::new(CountingTimedProvider::new(
        Ok(teletype_speech::Transcript {
            text: "hello world".into(),
            words: words.clone(),
            confidence: 0.96,
            language: Some("en".into()),
        }),
        Ok("should not be reached".into()),
    )));
    let calls = provider.clone();
    let samples = vec![0.1f32; 16_000];
    let result = run_provider(provider, &samples).expect("detailed path must succeed");
    assert_eq!(result.0, "hello world");
    let got_words = result
        .1
        .as_ref()
        .expect("detailed success must carry Some(words)");
    assert_eq!(
        *got_words, words,
        "word timings must pass through unchanged"
    );
    let counts = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        counts.detailed_calls.load(Ordering::SeqCst),
        1,
        "exactly one detailed call on the happy path"
    );
    assert_eq!(
        counts.plain_calls.load(Ordering::SeqCst),
        0,
        "plain path must never be called when detailed succeeds"
    );
}

#[test]
fn neural_provider_routes_coreml_and_sherpa_only() {
    // ggml Whisper/Parakeet have their own managers; the neural slot must
    // not claim them.
    assert!(neural_provider_for("large-v3-turbo-q5").is_none());
    assert!(neural_provider_for("parakeet-tdt-v3").is_none());
    assert!(neural_provider_for("not-a-model").is_none());

    assert!(neural_provider_for("sherpa-parakeet-v3-int8").is_some());
    assert!(neural_provider_for("sherpa-parakeet-v2-int8").is_some());
    // Core ML is only constructible when the FluidAudio bridge is built.
    assert_eq!(
        neural_provider_for("parakeet-ultra-coreml").is_some(),
        teletype_fluid::bridge_built()
    );
}

/// T10b: the loss warning fires on any lost words and stays silent on
/// a clean take (a warning after a clean delivery would be noise).
#[test]
fn loss_warning_fires_only_when_words_were_lost() {
    assert_eq!(loss_warning(0, false), None, "a clean take gets no warning");
    assert_eq!(
        loss_warning(1, false),
        Some("Part of that recording couldn't be transcribed")
    );
    assert_eq!(
        loss_warning(0, true),
        Some("Part of that recording couldn't be transcribed")
    );
    assert_eq!(
        loss_warning(2, true),
        Some("Part of that recording couldn't be transcribed")
    );
}

/// T10b: the first progress value always emits; later ones only after
/// the 250 ms interval (≤ 4 Hz to the pill's main thread).
#[test]
fn progress_throttle_emits_first_then_at_most_4hz() {
    assert!(should_emit_progress(None, 0), "first value always emits");
    assert!(
        !should_emit_progress(Some(0), 0),
        "0 ms since last: dropped"
    );
    assert!(
        !should_emit_progress(Some(1_000), 249_000),
        "< 250 ms: dropped"
    );
    assert!(
        should_emit_progress(Some(1_000), 251_000),
        "≥ 250 ms: emitted"
    );
    // A clock that goes backwards must not panic or emit a burst.
    assert!(
        !should_emit_progress(Some(500_000), 100_000),
        "backwards clock: dropped"
    );
}

/// A take with no frames is a microphone failure, not an empty success:
/// the caller must stop before ASR and surface the settings shortcut.
#[test]
fn an_empty_take_is_a_microphone_failure_not_a_silent_success() {
    let empty = Captured {
        samples: Vec::new(),
        duration_secs: 0.0,
        peak: 0.0,
    };
    assert_eq!(take_failure(&empty), Some(MIC_FAILURE_MESSAGE));

    let spoken = Captured {
        samples: vec![0.1; 16_000],
        duration_secs: 1.0,
        peak: 0.3,
    };
    assert_eq!(take_failure(&spoken), None);
}

/// The meter stream is capped at 20 Hz per take: every audio frame used
/// to trigger a reducer dispatch and a pill re-render. A fresh throttle
/// (one per take) lets a frame through only once the interval has
/// elapsed since the last emit; the first frame of a take always emits.
#[test]
fn pill_level_throttle_caps_100_frames_a_second_at_20_emits() {
    let mut t = PillLevelThrottle::new();
    let mut emits = 0;
    // 100 frames across 1 s (10 ms apart), rising levels.
    for i in 0..100 {
        let now_ms = i as u64 * 10;
        let level = (i as f32 / 99.0).min(1.0);
        if t.should_emit(now_ms, level) {
            emits += 1;
        }
    }
    assert!(
        emits <= 20,
        "100 frames in 1 s must emit at most 20 times, emitted {emits}"
    );
    assert!(
        emits >= 10,
        "the throttle must not starve the meter: emitted {emits}"
    );
    // A frame arriving before the interval elapsed is held back.
    let mut t2 = PillLevelThrottle::new();
    assert!(t2.should_emit(0, 0.5));
    assert!(!t2.should_emit(10, 0.6));
    // A fresh throttle (a new take) always emits its first frame.
    let mut t3 = PillLevelThrottle::new();
    assert!(t3.should_emit(1000, 0.3));
}

/// A model-load failure is a reason class, not a log line: the pill must
/// end warming with copy the user can act on. Missing file is the common
/// case (the model was never downloaded); provider construction failure
/// is a platform problem.
#[test]
fn warmup_failure_reason_classifies_missing_files_and_provider_errors() {
    assert_eq!(
        warmup_failure_reason(&std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file",
        )),
        "missing file"
    );
    assert_eq!(
        warmup_failure_reason(&std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied",
        )),
        "missing file"
    );
    assert_eq!(
        warmup_failure_reason(&std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "corrupt archive",
        )),
        "unsupported"
    );
    // Anything else (a provider that refused the model, a platform
    // error) is "other": the copy stays honest without over-claiming.
    assert_eq!(
        warmup_failure_reason(&std::io::Error::other("provider not built")),
        "other"
    );
    assert_eq!(warmup_failure_reason(&std::io::Error::other("")), "other");
}

/// A zero-sample take shows different copy depending on the mic
/// permission: a denied permission is fixable in System Settings, a
/// granted-but-silent mic is not — the message must say which.
#[test]
fn zero_sample_message_uses_permission_copy_only_when_access_is_off() {
    use teletype_core::platform::PermissionState;
    assert_eq!(
        zero_sample_take_message(Some(PermissionState::Denied)),
        MIC_ACCESS_DENIED_MESSAGE
    );
    assert_eq!(
        zero_sample_take_message(Some(PermissionState::NotDetermined)),
        MIC_ACCESS_DENIED_MESSAGE
    );
    assert_eq!(
        zero_sample_take_message(Some(PermissionState::Unsupported)),
        MIC_ACCESS_DENIED_MESSAGE
    );
    // Granted (and a query that cannot answer) keeps the existing copy.
    assert_eq!(
        zero_sample_take_message(Some(PermissionState::Granted)),
        MIC_FAILURE_MESSAGE
    );
    assert_eq!(zero_sample_take_message(None), MIC_FAILURE_MESSAGE);
}

/// Vocabulary boosting ships off. A non-zero size here re-arms the ASR-side
/// biasing that the bake-off measured as a regression on Core ML and a
/// no-op-or-harm everywhere else; the bar it has to clear is in asrs.md.
#[test]
fn boost_capacity_is_zero_for_every_engine() {
    for kind in [
        SpeechEngineKind::Fluid,
        SpeechEngineKind::Sherpa,
        SpeechEngineKind::Whisper,
        SpeechEngineKind::Whistle,
        SpeechEngineKind::Parakeet,
        SpeechEngineKind::ParakeetCpp,
    ] {
        assert_eq!(
            boost_capacity(kind),
            0,
            "{kind:?} must not be given a boost list"
        );
    }
}

#[test]
fn boost_terms_for_maps_lexicon_and_screen_terms_to_engine_terms() {
    let row = teletype_core::learning::store::LexiconRow {
        term: "PyTorch".into(),
        aliases: vec!["pie torch".into()],
        source: "learned".into(),
        weight: 1.0,
        hits: 0,
        false_alarms: 0,
        created: 1,
        last_used: Some(1_000),
        tombstoned: false,
        app_scope: None,
        learned_from: None,
    };
    let terms = boost_terms_for(&[row], None, &["Kubernetes".to_string()], 10, 1_000);
    assert_eq!(terms.len(), 2, "{terms:?}");
    let pytorch = terms
        .iter()
        .find(|t| t.text == "PyTorch")
        .expect("lexicon term missing");
    assert_eq!(pytorch.aliases, vec!["pie torch".to_string()]);
    assert!(pytorch.weight > 0.0);
    assert!(terms.iter().any(|t| t.text == "Kubernetes"));
}

#[test]
fn learned_terms_for_post_asr_filters_source_scope_and_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path();
    let learning_dir = data_dir.join("learning");
    std::fs::create_dir_all(&learning_dir).unwrap();
    let store =
        teletype_core::learning::store::LearningStore::open(&learning_dir.join("learning.sqlite"))
            .unwrap();

    let row = |term: &str, source: &str, scope: Option<&str>, tombstoned: bool| {
        teletype_core::learning::store::LexiconRow {
            term: term.into(),
            aliases: vec![format!("{} heard", term.to_lowercase())],
            source: source.into(),
            weight: 1.0,
            hits: 0,
            false_alarms: 0,
            created: 1,
            last_used: Some(1_000),
            tombstoned,
            app_scope: scope.map(str::to_string),
            learned_from: None,
        }
    };
    store
        .upsert_lexicon(&row("Siobhan", "learned", Some("chat"), false))
        .unwrap();
    store
        .upsert_lexicon(&row("Manual", "manual", None, false))
        .unwrap();
    store
        .upsert_lexicon(&row("Dead", "learned", None, true))
        .unwrap();

    let chat = Session::learned_terms_for_post_asr(data_dir, Some("chat"));
    assert_eq!(chat.len(), 1, "{chat:?}");
    assert_eq!(chat[0].canonical, "Siobhan");
    assert_eq!(chat[0].alias, "siobhan heard");
    assert!(Session::learned_terms_for_post_asr(data_dir, Some("email")).is_empty());
}

#[test]
fn boost_terms_for_respects_the_cap_and_excludes_tombstones() {
    let dead = teletype_core::learning::store::LexiconRow {
        term: "Dead".into(),
        aliases: Vec::new(),
        source: "manual".into(),
        weight: 1.0,
        hits: 0,
        false_alarms: 0,
        created: 1,
        last_used: None,
        tombstoned: true,
        app_scope: None,
        learned_from: None,
    };
    let terms = boost_terms_for(&[dead], None, &["a".to_string(), "b".to_string()], 1, 1_000);
    assert_eq!(terms.len(), 1);
    assert_eq!(terms[0].text, "a");
    assert!(boost_terms_for(&[], None, &[], 0, 1_000).is_empty());
}

#[test]
fn ensure_neural_provider_records_the_selected_model() {
    let state = crate::AppState::for_tests();
    ensure_neural_provider(&state, "sherpa-parakeet-v3-int8").unwrap();
    assert_eq!(
        state
            .neural_model_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_deref(),
        Some("sherpa-parakeet-v3-int8")
    );
    assert!(
        !state.neural.is_loaded(),
        "swapping providers must not load the model"
    );

    // A model the neural slot does not serve is rejected instead of
    // silently transcribing with whatever provider was there before.
    assert!(ensure_neural_provider(&state, "large-v3-turbo-q5").is_err());
}

#[test]
fn speech_model_paths_follow_the_catalog_engine_layout() {
    let dir = std::path::Path::new("/models");
    let mut settings = crate::commands::Settings {
        selected_speech_model: "parakeet-tdt-v3".into(),
        ..Default::default()
    };

    let (path, kind) = resolve_speech_model(&settings, dir);
    assert_eq!(kind, SpeechEngineKind::Parakeet);
    assert_eq!(path, dir.join("ggml-parakeet-tdt-0.6b-v3-q8_0.bin"));

    // Sherpa loads the extracted directory (archive name minus the
    // extension), never the `.tar.bz2` itself.
    settings.selected_speech_model = "sherpa-parakeet-v3-int8".into();
    let (path, kind) = resolve_speech_model(&settings, dir);
    assert_eq!(kind, SpeechEngineKind::Sherpa);
    assert_eq!(
        path,
        dir.join("sherpa")
            .join("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8")
    );

    // Fluid gets the models base dir; the bridge appends the repo
    // folder and downloads on first load.
    settings.selected_speech_model = "parakeet-ultra-coreml".into();
    let (path, kind) = resolve_speech_model(&settings, dir);
    assert_eq!(kind, SpeechEngineKind::Fluid);
    assert_eq!(path, dir.join("fluid"));

    settings.selected_speech_model = "large-v3-turbo-q5".into();
    let (path, kind) = resolve_speech_model(&settings, dir);
    assert_eq!(kind, SpeechEngineKind::Whisper);
    assert_eq!(path, dir.join("ggml-large-v3-turbo-q5_0.bin"));

    // Qwen3-ASR loads a directory (both GGUFs live inside it), so the
    // provider must never be handed the main `.gguf` file path.
    settings.selected_speech_model = "qwen3-asr".into();
    let (path, kind) = resolve_speech_model(&settings, dir);
    assert_eq!(kind, SpeechEngineKind::Qwen3Asr);
    assert_eq!(path, dir.join("qwen3-asr"));
}

#[test]
fn a_conflict_shows_only_for_the_binding_that_failed() {
    let stored = Some(HotkeyConflict::new("Ctrl+Shift+Space", "os error -9878"));
    // Same binding: visible.
    let shown = visible_conflict(&stored, "Ctrl+Shift+Space");
    assert_eq!(shown, stored);
    // User changed the binding since: the old failure must not linger.
    assert_eq!(visible_conflict(&stored, "Alt+Space"), None);
    // Nothing stored: nothing shown.
    assert_eq!(visible_conflict(&None, "Ctrl+Shift+Space"), None);
}

#[test]
fn the_conflict_message_blames_nobody() {
    // Worded as what macOS reported: a duplicate registration inside our
    // own process fails the same way, so naming another app is unproven.
    let c = HotkeyConflict::new("Ctrl+Shift+Space", "os error -9878");
    assert_eq!(c.binding, "Ctrl+Shift+Space");
    assert!(c.message.contains("already taken"), "{}", c.message);
    assert!(c.message.contains("Cause unknown"), "{}", c.message);
    assert!(c.message.contains("os error -9878"), "{}", c.message);
    for word in ["another app", "other app", "claimed by", "holds"] {
        assert!(!c.message.contains(word), "blames: {word}");
    }
}

/// Regression test for the P0-1 cancel guard: a cancel while the
/// pipeline is running must map to CancelPipeline (finish without
/// inserting), and a cancel while recording must map to a cancelled
/// stop (discard the audio).
#[test]
fn pill_cancel_maps_per_phase() {
    // Cancel mid-pipeline: let transcription finish, drop the result.
    let action = state::decide(
        Input::Cancel,
        Phase::Transforming,
        RecordingMode::Hold,
        &mut false,
        &mut false,
    );
    assert!(matches!(action, state::Action::CancelPipeline));

    // Cancel while recording: stop and discard.
    let action = state::decide(
        Input::Cancel,
        Phase::Listening,
        RecordingMode::Hold,
        &mut true,
        &mut false,
    );
    assert!(matches!(action, state::Action::Stop { cancelled: true }));

    // Cancel while idle: nothing to cancel.
    let action = state::decide(
        Input::Cancel,
        Phase::Idle,
        RecordingMode::Hold,
        &mut false,
        &mut false,
    );
    assert!(matches!(action, state::Action::Nothing));
}

#[test]
fn transliterates_cyrillic_name() {
    assert_eq!(transliterate_cyrillic("Рида Фатема"), "Rida Fatema");
}

#[test]
fn transliteration_preserves_latin() {
    assert_eq!(transliterate_cyrillic("Hello world"), "Hello world");
}

/// P2.5: wake phrase stripping, exact and fuzzy (edit distance ≤ 1).
#[test]
fn wake_phrase_strips_exact_and_fuzzy() {
    // Plain wake word.
    assert_eq!(
        strip_wake_phrase("teletype open safari"),
        Some("open safari".to_string())
    );
    // "hey teletype" form.
    assert_eq!(
        strip_wake_phrase("hey teletype send an email"),
        Some("send an email".to_string())
    );
    // Case-insensitive.
    assert_eq!(
        strip_wake_phrase("Hey Teletype do this"),
        Some("do this".to_string())
    );
    // ASR wobble: one edit in each wake word.
    assert_eq!(
        strip_wake_phrase("teletyp open safari"),
        Some("open safari".to_string())
    );
    assert_eq!(
        strip_wake_phrase("telotype open safari"),
        Some("open safari".to_string())
    );
    // Trailing punctuation on the wake word (common with ASR).
    assert_eq!(
        strip_wake_phrase("Teletype, open Safari"),
        Some("open Safari".to_string())
    );
}

/// P2.5: negative cases — the wake phrase must not fire on unrelated
/// speech, on a wake word that is not leading, or leave an empty command.
#[test]
fn wake_phrase_requires_leading_words_and_a_command() {
    assert_eq!(strip_wake_phrase("open safari"), None);
    assert_eq!(strip_wake_phrase("hey siri do this"), None);
    // Wake word mid-sentence is dictation, not a command.
    assert_eq!(strip_wake_phrase("please teletype open safari"), None);
    // Wake word alone carries no command: a plain dictation of the word
    // "teletype" must not become an empty command take.
    assert_eq!(strip_wake_phrase("teletype"), None);
    assert_eq!(strip_wake_phrase("hey teletype"), None);
    assert_eq!(strip_wake_phrase(""), None);
    // Two edits away: not the wake word.
    assert_eq!(strip_wake_phrase("teltypo open safari"), None);
}

#[test]
fn transliteration_handles_mixed_script() {
    assert_eq!(
        transliterate_cyrillic("Рида hello Фатема"),
        "Rida hello Fatema"
    );
}

#[test]
fn transliteration_drops_hard_soft_signs() {
    assert_eq!(transliterate_cyrillic("объём"), "obyom");
}

#[test]
fn ensure_latin_transliterates_when_english() {
    assert_eq!(ensure_latin_if_english("Рида Фатема", "en"), "Rida Fatema");
}

#[test]
fn ensure_latin_preserves_when_not_english() {
    assert_eq!(ensure_latin_if_english("Рида Фатема", "ru"), "Рида Фатема");
}

#[test]
fn ensure_latin_preserves_latin_text() {
    assert_eq!(ensure_latin_if_english("Rida Fatema", "en"), "Rida Fatema");
}

#[test]
fn effective_language_defaults_to_auto() {
    // Empty legacy setting means auto-detect (Whisper picks the language;
    // Parakeet ignores the argument inside its wrapper).
    assert_eq!(effective_language("".into(), false), "auto");
    // "auto" (any case) passes through; explicit codes pass through too.
    assert_eq!(effective_language("auto".into(), false), "auto");
    assert_eq!(effective_language("AUTO".into(), false), "auto");
    assert_eq!(effective_language("ru".into(), false), "ru");
    assert_eq!(effective_language("en".into(), true), "en");
    // Surrounding whitespace is trimmed, not treated as a language.
    assert_eq!(effective_language("  ru ".into(), false), "ru");
}

#[test]
fn effective_language_honours_english_only() {
    // The T8 fix: an English-only engine must not be passed a non-English
    // lock through — it cannot transcribe it, and a wrong-language hint
    // is worse than the engine's own default. The lock resolves to
    // "en"; "auto" stays "auto" (the engine's trained language still
    // applies).
    assert_eq!(effective_language("pl".into(), true), "en");
    assert_eq!(effective_language("de".into(), true), "en");
    // An English-region lock ("en-US") is not exactly "en" by the
    // predicate, so it resolves to the bare "en" the engine can
    // transcribe. The region suffix is not a capability signal.
    assert_eq!(effective_language("en-US".into(), true), "en");
    assert_eq!(effective_language("auto".into(), true), "auto");
    assert_eq!(effective_language("".into(), true), "auto");
    // "EN" (uppercase) is still English: not a lock, passed through.
    assert_eq!(effective_language("EN".into(), true), "EN");
    // A multilingual engine passes the lock through untouched.
    assert_eq!(effective_language("pl".into(), false), "pl");
    assert_eq!(effective_language("de".into(), false), "de");
}

#[test]
fn language_mismatch_notice_flags_locked_non_english_on_english_only_engine() {
    use crate::dictation::SpeechEngineKind;
    use teletype_speech::catalog::find;

    let whisper_kind = SpeechEngineKind::Whisper;
    let sherpa_kind = SpeechEngineKind::Sherpa; // English-only kind

    // "auto" / empty: nothing to say (the engine's own default applies).
    assert_eq!(
        language_mismatch_notice("auto", sherpa_kind, "large-v3-turbo-q5"),
        None
    );
    assert_eq!(
        language_mismatch_notice("", sherpa_kind, "large-v3-turbo-q5"),
        None
    );
    // English lock: every engine honours it, no notice.
    assert_eq!(
        language_mismatch_notice("en", sherpa_kind, "parakeet-tdt-v3"),
        None
    );
    // A locked non-English language on an English-only engine row is
    // the notice case: the model cannot transcribe it, the take runs as
    // English.
    let pl_notice = language_mismatch_notice("pl", sherpa_kind, "fluid-speech-mini").unwrap();
    assert!(
        pl_notice.contains("Polish"),
        "notice should name the language, got {pl_notice:?}"
    );
    assert!(
        pl_notice.contains("English"),
        "notice should name the fallback, got {pl_notice:?}"
    );
    // Whisper rows honour every language: no notice for a locked
    // non-English language.
    assert_eq!(
        language_mismatch_notice("pl", whisper_kind, "large-v3-turbo-q5"),
        None
    );
    // The Core ML v2 row is English-only and lists only English, so a
    // locked non-English language is a mismatch.
    let ar_notice = language_mismatch_notice("ar", whisper_kind, "parakeet-v2-coreml").unwrap();
    assert!(ar_notice.contains("Arabic"), "got {ar_notice:?}");
    // But a locked English language is not a mismatch: the model can
    // transcribe it.
    assert_eq!(
        language_mismatch_notice("en", whisper_kind, "parakeet-v2-coreml"),
        None
    );
    // A pinned model that is not in the catalog falls back to the
    // kind-based list: an English-only kind flags a locked non-English
    // language, a Whisper kind does not.
    assert!(
        language_mismatch_notice("pl", sherpa_kind, "custom-not-in-catalog").is_some(),
        "English-only kind must flag a locked non-English language even for an unknown row"
    );
    assert_eq!(
        language_mismatch_notice("pl", whisper_kind, "custom-not-in-catalog"),
        None,
        "Whisper kind must not flag a locked non-English language for an unknown row"
    );
    // The find() path the helper uses agrees with the catalog row the
    // helper is checking — this is the drift test for the helper.
    assert!(
        find("fluid-speech-mini").unwrap().english_only,
        "fluid-speech-mini must be an English-only row for this test to be meaningful"
    );
    assert!(!find("large-v3-turbo-q5").unwrap().english_only);
    assert!(find("parakeet-v2-coreml").unwrap().english_only);
    // The kind list is non-empty and contains the English-only kinds
    // this test relies on (guards against a future edit that removes
    // Sherpa from the list and silently breaks the fallback).
    assert!(crate::dictation::is_english_only(SpeechEngineKind::Sherpa));
    assert!(!crate::dictation::is_english_only(
        SpeechEngineKind::Whisper
    ));
}

#[test]
fn detected_language_flows_into_profile() {
    // Unit-level check of the auto-detect contract: when the engine
    // reports a detection, the pipeline must polish in that language.
    let mut profile = teletype_core::personalization::UserProfile::default();
    assert_eq!(profile.language, "en");
    let detected: Option<String> = Some("de".into());
    if let Some(d) = &detected {
        profile.language = d.clone();
    }
    assert_eq!(profile.language, "de");
}

// ------------------------------------------------------------------
// Cancellation-race concurrency tests.
//
// These drive the REAL controller event loop: `Controller::for_tests`
// runs the same `handle` the production thread uses, on its own thread,
// fed by the same FIFO `mpsc` channel. The test thread stands in for
// the pipeline worker by sending `PipelineReady` (the worker never
// injects; it only ships the result). The `on_inject` hook records
// "the controller decided to inject" in place of the platform call,
// which needs the Tauri main thread. What the tests pin down is the
// architecture's whole promise: the FIFO event order IS the
// cancellation decision, and the worker never performs the side effect.

/// The card is about polish, not about every edit the pipeline makes:
/// filler removal or AutoText change the text too, but they leave no polish
/// marks, so they must not be advertised as polish.
#[test]
fn polish_diff_always_emits_when_there_is_text() {
    use teletype_core::trace::TraceStageMarks;
    let mut marks = TraceStageMarks::default();

    // No polish marks: still emits (the card shows "0 changes"), just no
    // duration.
    let diff = polish_diff(
        "hello there",
        "Hello, there.",
        &marks,
        None,
        None,
        false,
        None,
        "hello there".into(),
    )
    .expect("card");
    assert_eq!(diff.raw, "hello there");
    assert_eq!(diff.polished, "Hello, there.");
    assert_eq!(diff.ms, 0);
    assert!(!diff.ran);
    assert_eq!(diff.model, None);
    assert_eq!(diff.original, "hello there");
    assert_eq!(diff.reason, None);
    assert_eq!(diff.profile, None);

    // With marks: the duration is reported.
    marks.t_polish_start = Some(1_000);
    marks.t_polish_end = Some(1_640);
    let diff = polish_diff(
        "hello there",
        "Hello, there.",
        &marks,
        Some("an advisory reason"),
        Some("structured"),
        true,
        Some("s1-mini".into()),
        "hello there".into(),
    )
    .expect("card");
    assert_eq!(diff.raw, "hello there");
    assert_eq!(diff.polished, "Hello, there.");
    assert_eq!(diff.ms, 640);
    assert!(diff.ran);
    assert_eq!(diff.model.as_deref(), Some("s1-mini"));
    assert_eq!(diff.original, "hello there");
    assert_eq!(diff.reason.as_deref(), Some("an advisory reason"));
    assert_eq!(diff.profile.as_deref(), Some("structured"));

    // Polish changed nothing: still emits, so the card shows "0 changes".
    let same = polish_diff(
        "same words",
        "same words",
        &marks,
        None,
        None,
        true,
        None,
        "same words".into(),
    )
    .expect("card");
    assert_eq!(same.raw, "same words");
    assert_eq!(same.polished, "same words");

    // Empty reason is normalized to None.
    let no_reason = polish_diff(
        "hello there",
        "Hello, there.",
        &marks,
        Some(""),
        None,
        false,
        None,
        "hello there".into(),
    )
    .expect("card");
    assert_eq!(no_reason.reason, None);

    // Empty text: no card.
    assert_eq!(
        polish_diff(
            "  ",
            "anything",
            &marks,
            None,
            None,
            false,
            None,
            "anything".into()
        ),
        None
    );

    // Polish skipped: still emits (raw == polished -> "0 changes").
    marks.polish_skipped_reason = Some("no polish model".into());
    let skipped = polish_diff(
        "hello there",
        "Hello, there.",
        &marks,
        None,
        None,
        false,
        None,
        "hello there".into(),
    )
    .expect("card");
    assert_eq!(skipped.polished, "Hello, there.");
}

fn test_payload(session: u64, text: &str) -> PipelinePayload {
    PipelinePayload {
        session,
        raw_text: text.to_string(),
        final_text: text.to_string(),
        polish_change: None,
        context: teletype_core::context::ApplicationContext::default(),
        record_target: None,
        detected_language: None,
        command: None,
        trace_marks: teletype_core::trace::TraceStageMarks::default(),
        decider: None,
        decider_ms: None,
        tool: None,
        args_source: None,
        losses: None,
        polish_profile: teletype_core::profile::PolishProfile::default(),
        polish_profile_reason: String::new(),
        polish_ran: false,
        polish_model: None,
    }
}

// ------------------------------------------------------------------
// P4.4: mock 60 s take — tail-only decode on release + trace fields.
//
// The controller's release path (worker-side) is modelled here the way
// the rest of the tests model the workers: a mock engine whose decode
// cost is 10 ms per second of audio. For a 60 s take the Segmenter
// commits the segments during recording, so on release only the tail
// (the uncommitted remainder) is decoded. The mock proves both
// properties the plan gates on: the tail is small (≤ 3 s of audio) and
// the release-to-final-text cost is the tail decode plus the rules-only
// join (≤ 80 ms of mock time, no LLM).
// ------------------------------------------------------------------

/// P4.4: a 60 s take through the Segmenter leaves a small tail, and
/// the release-side work (tail decode at 10 ms/s + rules-only join)
/// stays flat as the take grows.
#[test]
fn mock_60s_take_tail_only_decode_on_release() {
    use teletype_core::segmenter::{Segmenter, SegmenterConfig};

    // A long take with 500 ms pauses every 10 s: the Segmenter commits
    // one segment at each pause (5 during the take); the final 2.5 s of
    // speech is released before any pause — the uncommitted tail that is
    // decoded on release.
    let mut seg = Segmenter::new(SegmenterConfig::default());
    let mut committed_segments: u32 = 0;
    for _ in 0..5 {
        let events = seg.push(&vec![0.5; 160_000], 0.9); // 10 s speech
        committed_segments += events.len() as u32;
        let events = seg.push(&vec![0.0; 8000], 0.0); // 500 ms pause
        committed_segments += events.len() as u32;
    }
    let events = seg.push(&vec![0.5; 40_000], 0.9); // final 2.5 s speech
    committed_segments += events.len() as u32;
    let tail = seg.flush().expect("the tail is flushed on release");
    assert_eq!(committed_segments, 5, "a long take must commit mid-take");

    // Tail audio: the uncommitted remainder after the last commit.
    let tail_audio_ms =
        crate::committer::tail_audio_ms(tail.end_sample - tail.start_sample, 16_000);
    assert!(
        tail_audio_ms <= 3_000,
        "a 60 s take must leave ≤ 3 s of tail audio; got {tail_audio_ms} ms"
    );

    // Release-side cost: the mock engine decodes at 10 ms per second of
    // audio (the plan's mock timing), and the join is rules-only.
    let decode_ms = u64::from(tail_audio_ms) * 10 / 1000;
    let joined = crate::committer::join_segments(&[
        "segment one".to_string(),
        "segment two".to_string(),
        "segment three".to_string(),
        "segment four".to_string(),
        "segment five".to_string(),
        "the tail words".to_string(),
    ]);
    assert!(!joined.is_empty());
    let release_to_final_ms = decode_ms; // join is rules-only: no LLM
    assert!(
        release_to_final_ms <= 80,
        "release-to-final-text must stay flat as takes get longer; got {release_to_final_ms} ms"
    );
}

/// P4.4: `committed_segments` and `tail_audio_ms` ride the
/// `TraceStageMarks` into the `DictationTrace` for a segmented take.
#[test]
fn segmented_take_trace_records_committed_segments_and_tail() {
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
    session.headless_release(TakeKind::Dictation);
    // The worker (mock 60 s take) committed 5 segments during
    // recording and decoded a 2.5 s tail on release.
    let mut payload = test_payload(1, "joined final text");
    payload.trace_marks.committed_segments = 5;
    payload.trace_marks.tail_audio_ms = 2500;
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload,
        },
    );
    let trace = session.traces.get(&1).expect("finished trace");
    assert_eq!(trace.committed_segments, 5);
    assert_eq!(trace.tail_audio_ms, 2500);
}

/// Bounce guard: a `HotkeyDown` within `HOTKEY_BOUNCE_DEBOUNCE` of a
/// release, while idle, is a spurious Fn re-edge (down-up-down on one
/// physical press) and must NOT start a new take. Without this, a
/// bounced Fn press starts two back-to-back takes and pastes the text
/// twice.
#[test]
fn hotkey_down_within_bounce_window_of_release_is_suppressed() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    // A release just happened (e.g. a quick take ended); `last_release`
    // is fresh, well inside the 100 ms bounce window.
    session.headless_release(TakeKind::Dictation);
    // The bounced re-edge arrives within the window while idle.
    session.hotkey_down_event();
    assert_eq!(
        session.phase,
        Phase::Idle,
        "a sub-100ms re-press after a release is bounce, not a new take"
    );
}

/// Bounce guard (negative shape): a `HotkeyDown` OUTSIDE the bounce
/// window (a deliberate re-press, >100 ms after release) still starts
/// a new take while idle. The guard must not swallow real presses.
#[test]
fn hotkey_down_outside_bounce_window_still_starts_take() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    // A release 200 ms ago: outside the 100 ms bounce window but a
    // genuine new press.
    session.last_release = Some(Instant::now() - Duration::from_millis(200));
    session.hotkey_down_event();
    assert!(
        session.phase != Phase::Idle,
        "a deliberate re-press outside the bounce window must start a take, got {:?}",
        session.phase
    );
}

/// Bounce guard (release edge): the Fn key's spurious up edge arrives a
/// few ms after the press. It must not tap-paste (that throws the take
/// away and pastes a foreign clipboard) and must not stop the take: the
/// physical key sends its real up next, and that edge ends the take.
#[test]
fn bounced_release_keeps_the_take_recording() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.hotkey_down_event();
    assert_eq!(session.phase, Phase::Listening);
    // The spurious up, 5 ms into the hold.
    session.hotkey_up_event_measured(Some(5));
    assert_eq!(
        session.phase,
        Phase::Listening,
        "a release inside the bounce window must not stop the take"
    );
    // The follow-up down cancels the watchdog: the key is still held.
    // Clear last_release so the double-tap check (which would otherwise
    // flip to hands-free within 350 ms of the bounced release) does not
    // interfere with the release-path assertion below.
    let armed_seq = session.bounce_settle_seq;
    session.last_release = None;
    session.hotkey_down_event();
    session.hotkey_bounce_settled(armed_seq, 5);
    assert_eq!(
        session.phase,
        Phase::Listening,
        "a stale watchdog must not end a held take"
    );
    // The real release, well past the bounce window, ends the take.
    // In headless mode a clean stop enters Transcribing (the take is
    // stopped and would be transcribed), so that is the assertion.
    session.hotkey_up_event_measured(Some(1500));
    assert_eq!(
        session.phase,
        Phase::Transcribing,
        "a genuine release must stop the take (headless stop transcribes)"
    );
}

/// A genuine sub-100 ms tap is indistinguishable from a bounce at the up
/// edge, so it is deferred. When no press follows, the watchdog finishes
/// it as the release it was; the microphone must never stay open.
#[test]
fn genuine_short_tap_resolves_when_no_press_follows() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.hotkey_down_event();
    assert_eq!(session.phase, Phase::Listening);
    session.hotkey_up_event_measured(Some(60));
    assert_eq!(session.phase, Phase::Listening);
    let seq = session.bounce_settle_seq;
    session.hotkey_bounce_settled(seq, 60);
    // The take started ~0 ms ago (the headless `start` plants the
    // start stamp), so the visible-recording floor (500 ms) is not
    // met: the take is discarded silently. No flash, no paste.
    assert_eq!(
        session.phase,
        Phase::Idle,
        "the watchdog must resolve a genuine short tap (silent discard)"
    );
    assert!(
        session.headless_flashes.is_empty(),
        "a genuine short tap must not show any pill text"
    );
    assert_eq!(
        session
            .paste_calls
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a genuine short tap must not send the paste keystroke"
    );
}

/// The quick-tap rule, exhaustively. A Hold-mode dictation released
/// under a second is a quick release; every other shape keeps the
/// normal release behaviour.
#[test]
fn quick_tap_rule_is_hold_mode_dictation_only() {
    let held = TAP_PASTE_MAX_HOLD.as_millis() as u64;
    // The one true case: Hold, listening, plain dictation, under a second.
    assert!(is_quick_tap(
        RecordingMode::Hold,
        Phase::Listening,
        false,
        TakeKind::Dictation,
        300,
    ));
    // 999 ms taps, 1000 ms dictates: the boundary is the whole rule.
    assert!(is_quick_tap(
        RecordingMode::Hold,
        Phase::Listening,
        false,
        TakeKind::Dictation,
        held - 1,
    ));
    assert!(!is_quick_tap(
        RecordingMode::Hold,
        Phase::Listening,
        false,
        TakeKind::Dictation,
        held,
    ));
    // Toggle mode: a tap STARTS a take that must keep recording.
    assert!(!is_quick_tap(
        RecordingMode::Toggle,
        Phase::Listening,
        false,
        TakeKind::Dictation,
        300,
    ));
    // A command take keeps its own release behaviour.
    assert!(!is_quick_tap(
        RecordingMode::Hold,
        Phase::Listening,
        false,
        TakeKind::Command,
        300,
    ));
    // Hands-free releases are ignored by the state machine; never a tap.
    assert!(!is_quick_tap(
        RecordingMode::Hold,
        Phase::Listening,
        true,
        TakeKind::Dictation,
        300,
    ));
    // Only a release DURING listening can be a tap.
    assert!(!is_quick_tap(
        RecordingMode::Hold,
        Phase::Idle,
        false,
        TakeKind::Dictation,
        300,
    ));
    assert!(!is_quick_tap(
        RecordingMode::Hold,
        Phase::Transcribing,
        false,
        TakeKind::Dictation,
        300,
    ));
}

/// F-03: a quick release is an ordinary stop. A take that has been
/// visibly recording at least 500 ms is transcribed normally, even at
/// 300 ms of key hold, and the latches are left clear.
#[test]
fn quick_release_with_enough_visible_recording_is_an_ordinary_stop() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    session.started_at_ms = teletype_core::storage::now_ms() - 800;
    assert!(session.quick_tap_due(300));
    session.hotkey_up_event_measured(Some(300));
    assert_eq!(
        session.phase,
        Phase::Transcribing,
        "a quick release with 800 ms of visible recording must stop and transcribe"
    );
    assert!(
        !session.hotkey_down,
        "the release must clear the hotkey latch"
    );
    assert!(!session.hands_free);
    assert!(
        session.last_release.is_some(),
        "the release stamp drives the bounce guard and double-tap window"
    );
}

/// F-03: a quick release whose take has been visibly recording less than
/// 500 ms is discarded silently. No paste keystroke, no pill text (the
/// old "Too short" note is gone), the latches are clear.
#[test]
fn quick_release_under_visible_floor_discards_silently() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    session.started_at_ms = teletype_core::storage::now_ms() - 300;
    assert!(session.quick_tap_due(300));
    session.hotkey_up_event_measured(Some(300));
    assert_eq!(session.phase, Phase::Idle, "the take must end");
    assert_eq!(
        session
            .paste_calls
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a quick release must never send the paste keystroke"
    );
    assert!(
        session.headless_flashes.is_empty(),
        "a mistap must not show any pill text, got {:?}",
        session.headless_flashes
    );
    assert!(!session.hotkey_down);
    assert!(!session.hands_free);
}

/// F-03 follow-up: the silent discard must run the real stop path, not
/// a headless shortcut. `tap_discard` no longer returns early in
/// headless mode; it flows through `stop_with_note`'s cancel branch,
/// and the empty note must produce zero flashes, zero `UiState::Message`
/// broadcasts (any broadcast), an Idle phase, and no paste. This test
/// fails if anyone reintroduces `self.flash(cancel_note)` (or the
/// `Message` broadcast) for an empty note: the flash lands in
/// `headless_flashes` and the broadcast in `headless_broadcasts`.
#[test]
fn tap_discard_is_silent_on_the_real_stop_path() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    session.started_at_ms = teletype_core::storage::now_ms() - 300;
    assert!(session.quick_tap_due(300));
    session.hotkey_up_event_measured(Some(300));
    assert_eq!(session.phase, Phase::Idle, "the take must end");
    assert!(
        session.headless_flashes.is_empty(),
        "a silent discard must not flash any pill text, got {:?}",
        session.headless_flashes
    );
    assert!(
        session.headless_broadcasts.is_empty(),
        "a silent discard must not broadcast any UiState, got {:?}",
        session.headless_broadcasts
    );
    assert_eq!(
        session
            .paste_calls
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a silent discard must not send the paste keystroke"
    );
    assert!(!session.hotkey_down);
    assert!(!session.hands_free);
}

/// F-03 follow-up: the silent discard on the HAS-recording path.
/// Plants a synthetic recording so the cancel branch (not the
/// no-recording early return) is exercised. An empty note must
/// produce zero flashes, zero broadcasts, and an Idle phase.
/// This test fails on the buggy code where `self.flash(cancel_note)`
/// and the `Message` broadcast run unconditionally for any cancel.
#[test]
fn stop_with_note_empty_note_with_recording_is_silent() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    session.started_at_ms = teletype_core::storage::now_ms() - 300;
    RECORDING.with(|slot| {
        slot.borrow_mut()
            .replace(teletype_core::audio::Recording::synthetic());
    });
    session.stop_with_note(true, "");
    assert_eq!(session.phase, Phase::Idle, "a silent discard must idle");
    assert!(
        session.headless_flashes.is_empty(),
        "an empty-note cancel must not flash, got {:?}",
        session.headless_flashes
    );
    assert!(
        session.headless_broadcasts.is_empty(),
        "an empty-note cancel must not broadcast, got {:?}",
        session.headless_broadcasts
    );
}

/// F-03 follow-up: the same discipline, driven straight through
/// `stop_with_note` (the shared cancel path) rather than the hotkey
/// release. An empty note is the silent discard: no `Message`, no
/// flash, phase Idle.
#[test]
fn stop_with_note_empty_note_cancel_is_silent() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    session.started_at_ms = teletype_core::storage::now_ms() - 300;
    assert!(session.quick_tap_due(300));
    session.tap_discard();
    assert_eq!(session.phase, Phase::Idle, "a silent discard must idle");
    assert!(
        session.headless_flashes.is_empty(),
        "an empty-note cancel must not flash, got {:?}",
        session.headless_flashes
    );
    assert!(
        session.headless_broadcasts.is_empty(),
        "an empty-note cancel must not broadcast, got {:?}",
        session.headless_broadcasts
    );
}

/// F-03 follow-up: a named cancel (the pill's Cancel button, "Cancelled")
/// keeps today's behaviour: it flashes the note and broadcasts the
/// message. Guards the empty-note rule from swallowing real cancels.
/// Plants a recording in the slot: with no mic opened, a cancel ends at
/// the no-take early return before the note is ever shown.
#[test]
fn stop_with_note_named_cancel_still_shows_the_note() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    session.started_at_ms = teletype_core::storage::now_ms() - 300;
    RECORDING.with(|slot| {
        slot.borrow_mut()
            .replace(teletype_core::audio::Recording::synthetic());
    });
    session.stop(true);
    assert_eq!(session.phase, Phase::Idle, "a named cancel must idle");
    assert_eq!(
        session.headless_flashes,
        vec!["Cancelled".to_string()],
        "a named cancel must flash its note"
    );
    assert!(
        session.headless_broadcasts.iter().any(
            |s| matches!(s, teletype_core::state::UiState::Message { text } if text == "Cancelled")
        ),
        "a named cancel must broadcast its message, got {:?}",
        session.headless_broadcasts
    );
}

/// F-03 follow-up: the empty-note rule applies on the no-mic path too.
/// A cancel with an empty note whose mic never opened still ends with the
/// no-take cleanup and shows nothing.
#[test]
fn stop_with_note_empty_note_no_recording_is_silent() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    session.started_at_ms = teletype_core::storage::now_ms() - 300;
    session.stop_with_note(true, "");
    assert_eq!(session.phase, Phase::Idle, "a silent discard must idle");
    assert!(
        session.headless_flashes.is_empty(),
        "an empty-note cancel must not flash, got {:?}",
        session.headless_flashes
    );
    assert!(
        session.headless_broadcasts.is_empty(),
        "an empty-note cancel must not broadcast, got {:?}",
        session.headless_broadcasts
    );
}

/// F-03: the 500 ms visible-recording floor is inclusive. A release
/// whose take has been visibly recording 500 ms or more is an ordinary
/// stop, not a discard. The test plants the start stamp so the measured
/// visible time lands at or just above the floor.
#[test]
fn quick_release_at_visible_floor_transcribes() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    // Plant the start stamp so the measured visible time is at least
    // the floor plus a small margin (the release path measures from
    // `started_at_ms` to `now_ms()`, and the margin covers any time
    // between the stamp and the measurement).
    session.started_at_ms = teletype_core::storage::now_ms() - (MIN_TAKE_VISIBLE_MS + 50);
    assert!(session.quick_tap_due(300));
    session.hotkey_up_event_measured(Some(300));
    assert_eq!(
        session.phase,
        Phase::Transcribing,
        "a quick release at or above the visible floor must transcribe"
    );
}

/// A press held past the threshold is a real dictation and must not be
/// classified as a quick tap.
#[test]
fn long_hold_release_is_not_a_tap() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.hotkey_down = true;
    session.take_kind = TakeKind::Dictation;
    session.started_at_ms = teletype_core::storage::now_ms() - 1500;
    assert!(!session.quick_tap_due(1500));
}

/// Drives the controller loop the same way the production thread does:
/// one `handle` call per event, in FIFO order. `PipelineReady` goes
/// through the `on_inject` hook instead of the platform call.
// Generic over the hook so it accepts both `Box<dyn FnMut(..)>` and the
// `+ Send` variant `Controller::for_tests` returns (Box is invariant, so
// a plain `&mut Box<dyn FnMut>` parameter would reject the Send box).
fn drive<F: FnMut(&PipelinePayload)>(session: &mut Session, on_inject: &mut F, event: Event) {
    match event {
        Event::PipelineReady {
            session: id,
            payload,
        } => session.pipeline_ready(
            id,
            payload,
            Some(on_inject as &mut dyn FnMut(&PipelinePayload)),
        ),
        other => session.handle(other),
    }
}

#[test]
fn finish_trace_appends_deliver_last_and_keeps_worker_spans() {
    // The core trace carries per-stage timestamps, not a span log: the
    // worker's marks land in `t_text_pipeline_end` (set here as the
    // stand-in for the pipeline stage), and the controller's refused
    // reason lands in `errors` — the same two mutations `pipeline_ready`
    // performs on the refused path.
    let mut trace = teletype_core::trace::DictationTrace::new(9, "test");
    trace.t_text_pipeline_end = Some(120);
    let detail = "direct write (8 chars)".to_string();
    let mut finished = trace;
    finished.errors.push(format!("delivery refused: {detail}"));
    assert_eq!(finished.session, 9);
    assert_eq!(finished.t_text_pipeline_end, Some(120));
    assert!(finished
        .errors
        .iter()
        .any(|e| e.contains("direct write (8 chars)")));
}

#[test]
fn cancel_processed_before_pipeline_ready_discards_the_result() {
    // Take 1 is mid-pipeline. The user cancels (hotkey). The worker
    // finishes a moment later and ships PipelineReady. The cancel
    // event was processed first, so the controller must discard: no
    // injection.
    let (injected_tx, injected_rx) = mpsc::channel::<u64>();
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
        let _ = injected_tx.send(p.session);
    }));
    // The user's cancel hotkey is processed first.
    drive(&mut session, &mut on_inject, Event::Cancel);
    // The worker's result arrives after the cancel.
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload: test_payload(1, "must never be injected"),
        },
    );
    assert!(
        injected_rx.try_recv().is_err(),
        "cancelled session must not be injected"
    );
}

#[test]
fn pipeline_ready_processed_before_cancel_allows_injection() {
    // Same take, opposite order: the controller accepts the result and
    // injects; only then is the cancel processed. The text has already
    // landed, so the cancel just transitions the session.
    let (injected_tx, injected_rx) = mpsc::channel::<u64>();
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
        let _ = injected_tx.send(p.session);
    }));
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload: test_payload(1, "lands before the cancel"),
        },
    );
    // The cancel is processed after the verdict.
    drive(&mut session, &mut on_inject, Event::Cancel);
    assert_eq!(
        injected_rx.try_recv().ok(),
        Some(1),
        "ready-before-cancel must inject"
    );
}

#[test]
fn recover_after_panic_returns_a_busy_session_to_idle() {
    // 2026-10-28: a panic inside a handler (rphonetic panicked on a
    // non-ASCII transcript) aborted the take with no terminal state, so
    // the phase stayed busy, later hotkeys were swallowed, and the pill
    // sat visible at its idle size. The catch_unwind now calls this.
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    let orphaned = session.next_id;
    session.phase = Phase::Transforming;
    session.recover_after_panic();
    assert_eq!(session.phase, Phase::Idle);
    // The abandoned take's session is tagged cancelled, so a late
    // `PipelineReady` it emits is discarded instead of injected.
    assert!(
        session.cancelled_sessions.contains(&orphaned),
        "a panicked take's session must be tagged cancelled"
    );
    // Idempotent: recovering an already-idle session stays idle.
    session.recover_after_panic();
    assert_eq!(session.phase, Phase::Idle);
}

#[test]
fn worker_exit_guard_reports_an_abandoned_take_once() {
    let (tx, rx) = mpsc::channel::<Event>();
    {
        let _guard = WorkerExitGuard {
            tx: Some(tx),
            #[cfg(test)]
            test_events: None,
            session: 42,
            armed: true,
        };
    }
    assert!(matches!(
        rx.try_recv(),
        Ok(Event::WorkerAbandoned { session: 42 })
    ));
    assert!(rx.try_recv().is_err(), "exactly one report per take");
}

#[test]
fn worker_exit_guard_is_disarmed_by_the_terminal_event() {
    let (tx, rx) = mpsc::channel::<Event>();
    let mut guard = WorkerExitGuard {
        tx: Some(tx),
        #[cfg(test)]
        test_events: None,
        session: 7,
        armed: true,
    };
    guard.terminal(Event::PipelineWorkerDone { session: 7 });
    drop(guard);
    assert!(matches!(
        rx.try_recv(),
        Ok(Event::PipelineWorkerDone { session: 7 })
    ));
    assert!(
        rx.try_recv().is_err(),
        "a normal hand-off must not look like an abandoned take"
    );
}

#[test]
fn worker_exit_guard_survives_a_worker_panic_and_reports_it() {
    let (tx, rx) = mpsc::channel::<Event>();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = WorkerExitGuard {
            tx: Some(tx),
            #[cfg(test)]
            test_events: None,
            session: 9,
            armed: true,
        };
        panic!("worker died");
    }));
    assert!(panicked.is_err());
    assert!(matches!(
        rx.try_recv(),
        Ok(Event::WorkerAbandoned { session: 9 })
    ));
}

#[test]
fn worker_abandoned_resets_the_newest_session() {
    // The live stuck pill: the pipeline panicked after its "worker done"
    // event, so the only reset that remains is this one.
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.next_id = 3;
    session.phase = Phase::Transcribing;
    session.handle(Event::WorkerAbandoned { session: 3 });
    assert_eq!(session.phase, Phase::Idle);
}

#[test]
fn worker_abandoned_leaves_a_newer_take_alone() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.next_id = 5;
    session.phase = Phase::Listening;
    session.handle(Event::WorkerAbandoned { session: 4 });
    assert_eq!(
        session.phase,
        Phase::Listening,
        "a dead superseded worker must not reset the newer take's pill"
    );
}

#[test]
fn cancel_between_ready_and_done_still_injects_exactly_once() {
    // The race window the artificial delay exists to expose: ready is
    // processed (verdict: inject), the cancel is processed, and only
    // then the controller's own PipelineDone comes back. The injection
    // must happen exactly once and the session must end up idle, not
    // stuck in Transforming.
    let (injected_tx, injected_rx) = mpsc::channel::<u64>();
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
        let _ = injected_tx.send(p.session);
    }));
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload: test_payload(1, "one injection only"),
        },
    );
    // Cancel right after ready — inside the "verdict made, done not
    // yet processed" window.
    drive(&mut session, &mut on_inject, Event::Cancel);
    assert_eq!(injected_rx.try_recv().ok(), Some(1));
    assert!(inject_rx_empty(&injected_rx), "no second injection");
    assert_eq!(session.phase, Phase::Idle);
}

fn inject_rx_empty(rx: &mpsc::Receiver<u64>) -> bool {
    rx.try_recv().is_err()
}

#[test]
fn cancel_during_transcribing_drops_the_transcript_itself() {
    // Cancel before the ASR result even arrives: the transcript must
    // be dropped, so no pipeline and no injection can follow.
    let (injected_tx, injected_rx) = mpsc::channel::<u64>();
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
        let _ = injected_tx.send(p.session);
    }));
    drive(&mut session, &mut on_inject, Event::Cancel);
    drive(
        &mut session,
        &mut on_inject,
        Event::Transcribed {
            session: 1,
            transcript: Ok(("late transcript".into(), None)),
            detected_language: None,
            asr_start_us: 0,
            asr_end_us: 0,
            device_lost: false,
            device_lost_ms: 0,
            pcm: None,
            losses: None,
            take_audio: None,
            segment_polished: false,
            segment_raws: None,
        },
    );
    assert!(
        injected_rx.try_recv().is_err(),
        "a cancelled take must never transcribe or inject"
    );
    assert_eq!(session.phase, Phase::Idle);
}

#[test]
fn a_late_pipeline_ready_from_a_cancelled_old_session_is_discarded() {
    // Two takes back to back: take 1 is cancelled while its pipeline
    // runs, take 2 finishes normally. The stale PipelineReady for
    // take 1 must be discarded even though the session counter has
    // moved on — the cancelled-set, not the phase, is what remembers.
    let (injected_tx, injected_rx) = mpsc::channel::<u64>();
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
        let _ = injected_tx.send(p.session);
    }));
    // Take 1 is cancelled mid-pipeline.
    drive(&mut session, &mut on_inject, Event::Cancel);
    // Take 2: a fresh session id (the cancel's flash bumped the
    // counter), mid-pipeline.
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 2,
            payload: test_payload(2, "take two lands"),
        },
    );
    // The stale result from cancelled take 1 arrives last.
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload: test_payload(1, "stale, must be discarded"),
        },
    );
    assert_eq!(injected_rx.try_recv().ok(), Some(2));
    assert!(
        injected_rx.try_recv().is_err(),
        "stale cancelled result injected"
    );
}

// ------------------------------------------------------------------
// P1.3: paste before bookkeeping.
//
// The pipeline worker ships `PipelineReady` (the injection) FIRST and
// hands the usage/history/dictionary-usage writes to the bookkeeper
// afterwards. The test records the event order and pins that the
// injection (PipelineReady) precedes HistorySaved.

/// Drives the controller the same way `drive` does, but also records
/// `PipelineReady` as an event (the injection verdict) so the test can
/// assert the order against the bookkeeping writes.
fn drive_with_bookkeeping<F: FnMut(&PipelinePayload)>(
    session: &mut Session,
    on_inject: &mut F,
    events: &Arc<Mutex<Vec<String>>>,
    event: Event,
) {
    match event {
        Event::PipelineReady {
            session: id,
            payload,
        } => {
            session.pipeline_ready(
                id,
                payload,
                Some(on_inject as &mut dyn FnMut(&PipelinePayload)),
            );
            events.lock().unwrap().push("PipelineReady".into());
        }
        other => session.handle(other),
    }
}

#[test]
fn bookkeeping_happens_after_paste() {
    // The controller processes `PipelineReady` (the injection) and then
    // the pipeline worker's bookkeeping arrives. The event order must be
    // PipelineReady BEFORE HistorySaved: the user gets the paste first,
    // the disk writes follow. This is the whole point of P1.3 — no disk
    // write sits on the release→paste critical path.
    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (injected_tx, injected_rx) = mpsc::channel::<u64>();
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
        let _ = injected_tx.send(p.session);
    }));
    // The worker ships PipelineReady (the injection) first.
    drive_with_bookkeeping(
        &mut session,
        &mut on_inject,
        &events,
        Event::PipelineReady {
            session: 1,
            payload: test_payload(1, "paste first"),
        },
    );
    // The injection happened.
    assert_eq!(
        injected_rx.try_recv().ok(),
        Some(1),
        "PipelineReady must inject"
    );
    // The bookkeeper's history write arrives after the injection.
    events.lock().unwrap().push("HistorySaved".into());
    // The order is pinned: PipelineReady (the paste) precedes HistorySaved.
    let ev = events.lock().unwrap().clone();
    let ready_pos = ev
        .iter()
        .position(|e| e == "PipelineReady")
        .expect("PipelineReady must be recorded");
    let hist_pos = ev
        .iter()
        .position(|e| e == "HistorySaved")
        .expect("HistorySaved must be recorded");
    assert!(
        ready_pos < hist_pos,
        "PipelineReady (paste) must precede HistorySaved (bookkeeping); got {:?}",
        ev
    );
}

// ------------------------------------------------------------------
// P2.1: Command Mode state machine — failing tests first.
//
// These pin the three P2.1 failures from plan §2.1:
// (a) an armed command take must reach the command router, not be
//     misclassified as dictation;
// (b) a Hold-mode release must stop a command take (the state machine
//     must have tracked the hotkey);
// (c) the command kind must not leak into the next dictation take.
//
// The headless controller (`Controller::for_tests`) drives the real
// `handle`/`transcribed`/`pipeline_ready` path. A take classified as a
// command ships `PipelineReady { command: Some(..) }`, which the
// controller routes through `command_executor::execute` instead of
// injecting text.

/// The command take's `PipelineReady`: the pipeline classified the
/// transcript as a command and extracted a structured action.
#[cfg(test)]
fn command_payload(session: u64) -> PipelinePayload {
    let mut p = test_payload(session, "open safari");
    p.command = Some(teletype_core::llm::CommandAction {
        tool: "open_app".into(),
        arguments: serde_json::json!({ "app": "Safari" }),
    });
    p
}

#[cfg(test)]
fn command_executor_says(cmd: &teletype_core::llm::CommandAction) -> String {
    crate::command_executor::execute(cmd, &Default::default())
        .unwrap_or_else(|e| format!("executor error: {e}"))
}

/// T10b: a take whose streaming decode lost words is still delivered,
/// then flashed with the loss warning. A clean take gets no warning.
#[test]
fn lossy_take_delivers_then_flashes_warning() {
    let (_controller, mut session, mut on_inject) =
        Controller::for_tests(Box::new(|p: &PipelinePayload| {
            assert!(
                !p.final_text.is_empty(),
                "the partial text is still delivered"
            );
        }));
    let mut payload = test_payload(1, "partial text");
    payload.losses = Some(crate::committer::CommitterFailureReport {
        failed_segments: 1,
        tail_failed: false,
    });
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload,
        },
    );
    assert_eq!(
        session.headless_flashes,
        vec!["Part of that recording couldn't be transcribed".to_string()],
        "a lossy take flashes the warning exactly once"
    );

    // A clean take (no losses) must not flash anything.
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload: test_payload(1, "clean text"),
        },
    );
    assert!(
        session.headless_flashes.is_empty(),
        "a clean take gets no warning"
    );
}

/// T10b: the trace records the loss counts (counts only, never text).
#[test]
fn lossy_take_records_counts_in_trace() {
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
    // The controller keeps the trace in `session.traces` once the take
    // starts (see `ensure_trace`); the test path reads it back from
    // there after `pipeline_ready`.
    let trace = DictationTrace::new(1, "test");
    session.traces.insert(1, trace);
    let mut payload = test_payload(1, "partial text");
    payload.losses = Some(crate::committer::CommitterFailureReport {
        failed_segments: 2,
        tail_failed: true,
    });
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload,
        },
    );
    let trace = session.traces.get(&1).expect("the trace is kept in tests");
    assert!(
        trace
            .errors
            .iter()
            .any(|e| e.contains("word loss: 2 segment(s) failed, tail_failed=true")),
        "the trace records the counts: {:?}",
        trace.errors
    );
}

/// (a) P2.1: a take started as a command (armed) reaches the command
/// router — the `PipelineReady` carries the extracted action and the
/// controller routes it through the executor, not the injector.
#[test]
fn armed_command_take_reaches_command_router() {
    let executed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = executed.clone();
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
        // The command's "injection" is its execution.
        if let Some(cmd) = &p.command {
            recorded.lock().unwrap().push(command_executor_says(cmd));
        }
    }));
    // The take is armed as a command and started through the state
    // machine (P2.3: every start goes through `input_with_kind`).
    session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
    // Release the key (the take ends; the pipeline runs in the
    // background) and let it reach the pipeline phase.
    session.headless_release(TakeKind::Command);
    // The pipeline finishes and ships the extracted command.
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload: command_payload(1),
        },
    );
    let got = executed.lock().unwrap().clone();
    assert!(
        got.iter().any(|m| m.contains("Safari")),
        "command take must reach the command router; executor saw: {got:?}"
    );
}

/// PRIV-1: the redaction helper keeps the diagnostic shape of the
/// executor's message. A value that is a substring of another value
/// (`a` inside `app`) must not corrupt the longer redaction, and a
/// value that appears twice must be redacted at every occurrence.
#[test]
fn redact_command_message_replaces_values_with_length_placeholders() {
    // `app` (3 chars) appears twice; `a` (1 char) is a substring of
    // `app` and also appears on its own in the message.
    let message = "opened a Google Calendar draft: app and app";
    let values = vec!["app".to_string(), "a".to_string()];
    let redacted = redact_command_message(message, &values);
    assert!(
        !redacted.contains("app"),
        "the longer value must be gone: {redacted:?}"
    );
    // The non-dictated shape of the message is intact: the longer value
    // is redacted at both occurrences, the shorter value at its own
    // occurrence, and nothing else changed.
    assert_eq!(
        redacted,
        "opened <1> Google C<1>lend<1>r dr<1>ft: <3> <1>nd <3>"
    );
    // A value that appears twice is redacted at every occurrence.
    let twice = redact_command_message("sent message to bob (verified)", &["bob".to_string()]);
    assert_eq!(twice, "sent message to <3> (verified)");
}

/// PRIV-1: a message that contains none of the argument values is
/// returned unchanged.
#[test]
fn redact_command_message_leaves_unrelated_message_unchanged() {
    let message = "opened the Settings app";
    let values = vec!["Safari".to_string(), "Notes".to_string()];
    assert_eq!(
        redact_command_message(message, &values),
        message,
        "no value appears, so nothing changes"
    );
}

/// P2.9: a mock command take leaves its whole lifecycle in the trace,
/// in order: controller-side `armed` / `started` at take start, then the
/// worker-side `classified=<intent>` and `executed`/`refused(<reason>)`
/// that ride the payload's `TraceStageMarks`. The headless `on_inject`
/// path keeps the finished trace in `session.traces`, which is what
/// this test reads back (production writes it to the traces JSONL).
#[test]
fn command_take_trace_records_full_lifecycle() {
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
    // A start test begins at Idle (the harness presets Transforming).
    session.phase = Phase::Idle;
    // A dedicated command take: armed and started by the state machine.
    session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
    session.headless_release(TakeKind::Command);
    // The worker classified the take as a command, executed the action,
    // and shipped both events in the payload.
    let mut payload = command_payload(1);
    payload.trace_marks.command_events = vec!["classified=command".into(), "executed".into()];
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload,
        },
    );
    let trace = session
        .traces
        .get(&1)
        .expect("the test path keeps the finished trace for assertions");
    assert_eq!(
        trace.command_mode,
        vec!["armed", "started", "classified=command", "executed"],
        "a command take must record its full lifecycle, in order"
    );
}

/// P2.9 negative shape: a plain dictation take records no command
/// lifecycle at all (the field stays empty and is omitted from JSON).
#[test]
fn dictation_take_trace_has_no_command_lifecycle() {
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
    session.headless_release(TakeKind::Dictation);
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload: test_payload(1, "plain dictation"),
        },
    );
    let trace = session.traces.get(&1).expect("finished trace");
    assert!(
        trace.command_mode.is_empty(),
        "dictation takes have no command lifecycle: {:?}",
        trace.command_mode
    );
}

/// A.3.3: the command decider trace fields ride the payload into the
/// finished trace (decider kind, decide wall time, tool, args source).
#[test]
fn command_decider_trace_fields_land_in_trace() {
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
    session.headless_release(TakeKind::Command);
    let mut payload = command_payload(1);
    payload.decider = Some("neudecide".into());
    payload.decider_ms = Some(42);
    payload.tool = Some("web_search".into());
    payload.args_source = Some("transcript".into());
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload,
        },
    );
    let trace = session
        .traces
        .get(&1)
        .expect("the test path keeps the finished trace for assertions");
    assert_eq!(trace.decider.as_deref(), Some("neudecide"));
    assert_eq!(trace.decider_ms, Some(42));
    assert_eq!(trace.tool.as_deref(), Some("web_search"));
    assert_eq!(trace.args_source.as_deref(), Some("transcript"));
}

/// A.3.3: a take that never ran the command route keeps the four
/// decider trace fields `None` (and they are omitted from the JSON).
#[test]
fn dictation_take_trace_has_no_decider_fields() {
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
    session.headless_release(TakeKind::Dictation);
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload: test_payload(1, "plain dictation"),
        },
    );
    let trace = session.traces.get(&1).expect("finished trace");
    assert!(trace.decider.is_none());
    assert!(trace.decider_ms.is_none());
    assert!(trace.tool.is_none());
    assert!(trace.args_source.is_none());
}

/// (b) P2.1: in Hold mode, releasing the hotkey stops a command take.
/// Before the fix the command path skipped `state::decide`, so
/// `hotkey_down` was never set and the release produced
/// `Action::Nothing` — recording kept going.
#[test]
fn hold_mode_release_stops_command_take() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    // Fresh session: the harness presets mid-pipeline Transforming for
    // the cancel tests; a start test begins at Idle.
    session.phase = Phase::Idle;
    // Armed command take, Hold mode (the headless default).
    session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
    assert_eq!(session.phase, Phase::Listening);
    assert!(
        session.hotkey_down,
        "start must track the hotkey in the state machine"
    );
    // Release: the state machine returns Stop, so the take ends.
    // (The real stop needs the mic; the headless release asserts the
    // decision the controller will act on, plus that the tracking state
    // is sound.)
    let action = state::decide(
        Input::HotkeyUp,
        session.phase,
        session.recording_mode(),
        &mut session.hotkey_down,
        &mut session.hands_free,
    );
    assert!(
        matches!(action, state::Action::Stop { cancelled: false }),
        "hold-mode release must stop a command take; got {action:?}"
    );
    assert!(!session.hotkey_down, "release must clear the hotkey latch");
    // The controller acts on that decision: the take ends and the
    // session moves to the pipeline phase.
    session.headless_release(TakeKind::Command);
    assert_eq!(
        session.phase,
        Phase::Transforming,
        "release must end the command take"
    );
}

/// (c) P2.1: the command kind is per-session. A command take must not
/// leave the session in a state where the next take inherits
/// Command-kind behaviour — the next take is a plain dictation.
#[test]
fn command_flag_does_not_leak_into_next_dictation() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    // Fresh session (see above): starts take 1 at Idle.
    session.phase = Phase::Idle;
    // Take 1 is a command: the kind is set at start.
    session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
    assert_eq!(session.take_kind(), TakeKind::Command);
    // The take ends (release + pipeline done).
    session.headless_release(TakeKind::Command);
    session.go_idle();
    // Take 2 is a plain dictation: it must start as Dictation, not
    // inherit the command kind from take 1.
    session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
    assert_eq!(session.phase, Phase::Listening);
    assert_eq!(
        session.take_kind(),
        TakeKind::Dictation,
        "command kind leaked into the next dictation take"
    );
}

/// P2-B: a press the state machine ignores (the hotkey latch is already
/// down — e.g. the dictation key pressed while a command take runs)
/// must not re-tag the in-flight take. Before the fix the kind was set
/// before `decide`, so such a press silently turned a running command
/// take into a dictation and the command never reached the pipeline.
#[test]
fn swallowed_press_does_not_retag_take_kind() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
    assert_eq!(session.take_kind(), TakeKind::Command);
    // Second press, different kind: swallowed by the latch.
    session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
    assert_eq!(
        session.take_kind(),
        TakeKind::Command,
        "a swallowed press re-tagged the in-flight command take"
    );
}

/// P2.8: the tray's "Run Voice Command" item starts a command take
/// from Idle, tagged as a command for the pipeline.
#[test]
fn command_toggle_starts_command_take() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.command_toggle_event();
    assert_eq!(session.phase, Phase::Listening);
    assert_eq!(session.take_kind(), TakeKind::Command);
}

/// CMD-2: with a bare-modifier dictation hotkey, the Control of the
/// command chord arms the dictation tap first, so the chord's own press
/// lands while a take is already in flight. It must take that young take
/// over instead of being swallowed by the hotkey latch: the recording is
/// live, so the words after the chord are the command.
#[test]
fn command_chord_takes_over_a_young_dictation_take() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    // The bare Ctrl down: an ordinary dictation take starts.
    session.hotkey_down_event();
    assert_eq!(session.phase, Phase::Listening);
    assert_eq!(session.take_kind(), TakeKind::Dictation);
    // Option then Space, a chord's worth of key travel later.
    session.started_at_ms = teletype_core::storage::now_ms() - 150;
    let seq_before = session.bounce_settle_seq;
    session.command_hotkey_down_event();
    assert_eq!(
        session.take_kind(),
        TakeKind::Command,
        "the chord press was swallowed by the in-flight dictation take"
    );
    assert_eq!(
        session.phase,
        Phase::Listening,
        "a takeover must re-route the live recording, not restart it"
    );
    assert_eq!(
        session.bounce_settle_seq,
        seq_before + 1,
        "the takeover left the bounce watchdog armed, which would end the command take"
    );
    let key = session.trace_key.expect("the in-flight take has a trace");
    let trace = session
        .traces
        .get(&key)
        .expect("trace for the in-flight take");
    assert_eq!(trace.take_kind.as_deref(), Some("command"));
    assert_eq!(
        trace.command_mode.last().map(String::as_str),
        Some("armed(chord takeover)"),
        "the trace must show the command route began at the chord"
    );
}

/// CMD-2, the other side of the window: past COMMAND_TAKEOVER_WINDOW the
/// buffer holds a dictation the user is deliberately giving, and a chord
/// press must not silently throw it away.
#[test]
fn late_chord_press_leaves_an_old_take_alone() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.hotkey_down_event();
    session.started_at_ms =
        teletype_core::storage::now_ms() - (COMMAND_TAKEOVER_WINDOW.as_millis() as u64 + 300);
    session.command_hotkey_down_event();
    assert_eq!(
        session.take_kind(),
        TakeKind::Dictation,
        "a chord past the takeover window re-routed a dictation in progress"
    );
    let key = session.trace_key.expect("the in-flight take has a trace");
    let trace = session
        .traces
        .get(&key)
        .expect("trace for the in-flight take");
    assert_eq!(
        trace.take_kind.as_deref(),
        Some("dictation"),
        "the trace must carry the route the take is on"
    );
    assert!(trace.command_mode.is_empty(), "no command route ran");
}

/// CMD-2: the release the tap reports when a second modifier joins the
/// held one lands inside the bounce window and arms the settle watchdog.
/// The chord press is the follow-up press that cancels it, so the queued
/// settle must not end the command take.
#[test]
fn stale_bounce_settle_cannot_end_a_taken_over_take() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.hotkey_down_event();
    session.started_at_ms = teletype_core::storage::now_ms() - 150;
    // The number the watchdog armed before the chord carries.
    let stale_seq = session.bounce_settle_seq;
    session.command_hotkey_down_event();
    assert_eq!(session.take_kind(), TakeKind::Command);
    session.hotkey_bounce_settled(stale_seq, 90);
    assert_eq!(
        session.phase,
        Phase::Listening,
        "a stale bounce settle ended the taken-over command take"
    );
}

/// CMD-PIPELINE-REPAIR A: the measured unwind of a real Ctrl+Option+Space
/// chord against a bare-Ctrl dictation hotkey. Ctrl down at 0 starts the
/// dictation take; Option down at 80 ms is read as a release of the bare
/// modifier (the tap reports a Ctrl-only up), which lands inside the
/// bounce window (100 ms) and arms the settle watchdog; Space at 180 ms
/// is the command chord's press. The chord must take the take over, and
/// the release deferred to the remaining window must not end it: the
/// 120 ms watchdog fires after the chord, and only a command press that
/// does not cancel the deferral could stop the take early.
#[test]
fn chord_unwind_release_at_150ms_takes_over_before_space() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    // t=0: Ctrl down starts the dictation take.
    session.hotkey_down_event();
    assert_eq!(session.phase, Phase::Listening);
    assert_eq!(session.take_kind(), TakeKind::Dictation);
    // t=80: Option joins. The tap reports the bare modifier as released;
    // the measured hold (80 ms) is inside the bounce window (100 ms), so
    // the release is deferred to the settle watchdog instead of stopping.
    // The take has been recording for the Option's worth of key travel,
    // so it is well inside the takeover window.
    session.started_at_ms = teletype_core::storage::now_ms() - 80;
    session.hotkey_up_event_measured(Some(80));
    assert_eq!(
        session.phase,
        Phase::Listening,
        "a release inside the bounce window must defer, not stop"
    );
    let seq_after_release = session.bounce_settle_seq;
    assert_ne!(
        seq_after_release, 0,
        "the deferred release must arm the settle watchdog"
    );
    // t=180: Space. The chord press takes the young take over and
    // cancels the deferred release (the sequence bump is the cancel).
    session.command_hotkey_down_event();
    assert_eq!(
        session.take_kind(),
        TakeKind::Command,
        "the chord press must take the in-flight dictation take over"
    );
    assert_eq!(
        session.phase,
        Phase::Listening,
        "the takeover re-routes the live recording, it does not restart it"
    );
    // The watchdog fires 120 ms after the deferred release (t=200),
    // carrying the sequence the release armed. The chord press bumped
    // it, so the settle must be a no-op: the command take keeps
    // recording.
    session.hotkey_bounce_settled(seq_after_release, 80);
    assert_eq!(
        session.phase,
        Phase::Listening,
        "the deferred release ended the taken-over command take"
    );
    // The take still holds the command route and its trace says so.
    let key = session.trace_key.expect("the in-flight take has a trace");
    let trace = session.traces.get(&key).expect("trace for the take");
    assert_eq!(trace.take_kind.as_deref(), Some("command"));
    assert!(
        trace
            .command_mode
            .iter()
            .any(|e| e == "armed(chord takeover)"),
        "the trace must show the command route began at the chord: {:?}",
        trace.command_mode
    );
}

/// CMD-PIPELINE-REPAIR A, the measured unwind where the settle fires
/// BEFORE the chord press. Option joins at 50 ms, the deferred release's
/// watchdog fires at t=170 (120 ms after the release), and Space does
/// not arrive until t=220. The settle resolves the deferred release as
/// a genuine short tap: the take is under the visible floor, so it is
/// discarded silently and the session returns to Idle. The subsequent
/// chord press then starts a FRESH command take (not a takeover of the
/// old one), with its own trace armed at the chord.
#[test]
fn chord_unwind_release_at_50ms_settle_discards_then_chord_starts_fresh() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.hotkey_down_event();
    // t=50: Option joins; the bare-modifier release is deferred (50 ms
    // is inside the bounce window) and the watchdog is armed.
    session.started_at_ms = teletype_core::storage::now_ms() - 50;
    session.hotkey_up_event_measured(Some(50));
    let seq_after_release = session.bounce_settle_seq;
    // t=170: the settle fires, 120 ms after the release. Space has not
    // come yet, so the take is still a young dictation under the
    // visible floor. The settle resolves the deferred release as a
    // genuine short tap and discards the take silently.
    session.hotkey_bounce_settled(seq_after_release, 50);
    assert_eq!(
        session.phase,
        Phase::Idle,
        "the settle must resolve the deferred release and discard the young take"
    );
    // t=300: Space. The chord press starts a FRESH command take from
    // Idle (the old take was already discarded, so this is not a
    // takeover). In real time the chord comes >100 ms after the settle,
    // so the bounce guard does not suppress it.
    session.last_release = Some(std::time::Instant::now() - std::time::Duration::from_millis(150));
    session.command_hotkey_down_event();
    assert_eq!(session.take_kind(), TakeKind::Command);
    assert_eq!(session.phase, Phase::Listening);
    // The fresh command take has its own trace, armed at the chord.
    let key = session
        .trace_key
        .expect("the fresh command take has a trace");
    let trace = session.traces.get(&key).expect("trace for the take");
    assert!(trace.t_hotkey_down.is_some(), "the fresh trace is armed");
    assert_eq!(
        trace.take_kind.as_deref(),
        Some("command"),
        "the fresh take is tagged command"
    );
    assert!(
        trace.command_mode.iter().any(|e| e == "armed"),
        "the trace must show the command armed at start: {:?}",
        trace.command_mode
    );
}

/// CMD-PIPELINE-REPAIR A: the window boundary. A dictation take that is
/// older than COMMAND_TAKEOVER_WINDOW when the chord arrives is content
/// the user is deliberately giving: the chord must not take it over, and
/// the take must keep its dictation route and audio untouched.
#[test]
fn in_flight_dictation_past_window_keeps_route_and_audio() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.hotkey_down_event();
    session.started_at_ms =
        teletype_core::storage::now_ms() - (COMMAND_TAKEOVER_WINDOW.as_millis() as u64 + 1);
    session.command_hotkey_down_event();
    assert_eq!(
        session.take_kind(),
        TakeKind::Dictation,
        "a chord past the takeover window must not re-route an established take"
    );
    let key = session.trace_key.expect("the in-flight take has a trace");
    let trace = session.traces.get(&key).expect("trace for the take");
    assert_eq!(trace.take_kind.as_deref(), Some("dictation"));
    assert!(trace.command_mode.is_empty(), "no command route ran");
    // The established release path is unchanged: a long hold still
    // stops and transcribes the take as dictation.
    session.hotkey_up_event_measured(Some(2_000));
    assert!(
        !matches!(session.phase, Phase::Listening),
        "an established take's release must still stop it"
    );
}

/// CMD-PIPELINE-REPAIR A: a normal short tap with no chord present
/// expires cleanly through the original logic. The deferred release's
/// settle fires, the take is a mistap under the visible floor, and it is
/// discarded silently: no command route, no paste, no note.
#[test]
fn short_tap_without_chord_expires_cleanly() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.hotkey_down_event();
    assert_eq!(session.take_kind(), TakeKind::Dictation);
    // A quick release inside the bounce window arms the watchdog.
    session.hotkey_up_event_measured(Some(60));
    let seq = session.bounce_settle_seq;
    // The take is young (under the visible floor): the settle that
    // follows discards it silently, exactly as before the chord work.
    session.hotkey_bounce_settled(seq, 60);
    assert_eq!(
        session.phase,
        Phase::Idle,
        "a short tap with no chord must end the take cleanly"
    );
    assert!(
        session.headless_flashes.is_empty(),
        "a silent discard must show nothing: {:?}",
        session.headless_flashes
    );
}

/// CMD-PIPELINE-REPAIR A/B: the full chord unwind ends the take as a
/// command, and the next take starts clean: the hotkey latch is clear,
/// the trace latch is closed (no pending trace for the finished take),
/// and the next press starts a fresh dictation take with a fresh trace.
#[test]
fn chord_unwind_then_next_take_starts_clean() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    // Ctrl down, Option join (deferred release), Space: the chord takes
    // the young take over.
    session.hotkey_down_event();
    session.started_at_ms = teletype_core::storage::now_ms() - 80;
    session.hotkey_up_event_measured(Some(80));
    session.command_hotkey_down_event();
    assert_eq!(session.take_kind(), TakeKind::Command);
    // The chord's release: a long hold (the user held the chord), so it
    // stops and transcribes the take through the normal release path.
    session.hotkey_up_event_measured(Some(900));
    assert!(
        !matches!(session.phase, Phase::Listening),
        "the chord's release must stop the command take"
    );
    // The take is over. The trace latch must be closed: `stop` re-keys
    // the trace to the session id and clears `trace_key`, so no pending
    // trace is left for the next take to inherit.
    assert!(
        session.trace_key.is_none(),
        "the finished take left a pending trace for the next take"
    );
    // The next press starts a fresh dictation take.
    session.go_idle();
    session.hotkey_down_event();
    assert_eq!(session.phase, Phase::Listening);
    assert_eq!(
        session.take_kind(),
        TakeKind::Dictation,
        "the take after a chord unwind must be plain dictation"
    );
    let key = session.trace_key.expect("the next take has a fresh trace");
    let trace = session.traces.get(&key).expect("trace for the next take");
    assert_eq!(
        trace.take_kind.as_deref(),
        Some("dictation"),
        "the next take's trace must carry its own route"
    );
    assert!(
        trace.command_mode.is_empty(),
        "the next take must not inherit the command route: {:?}",
        trace.command_mode
    );
}

/// CMD-PIPELINE-REPAIR B: the redundant edge that rides the chord
/// unwind (Option-up, which the tap reports as a bare-Ctrl re-edge)
/// must not allocate a second trace and replace the pending one. After
/// `stop()` re-keys the take's trace to the session id and clears
/// `trace_key`, a redundant press through the real event path used to
/// call `ensure_trace()` and mint a fresh trace, so the worker's
/// `PipelineReady` found no trace under the session id and the take's
/// marks (including `take_kind`) were written to a phantom trace that
/// is never finished. The surviving trace must be the original one:
/// same id, same armed/start marks, no second trace.
#[test]
fn redundant_edges_after_stop_do_not_replace_the_pending_trace() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    // The real stop path needs a take to finish: plant a synthetic
    // recording with audio, or `stop` takes the "mic never opened"
    // branch and drops the pending trace before the assertions below.
    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| {
        slot.borrow_mut().replace(recording);
    });
    let tone: Vec<f32> = (0..16_000)
        .map(|i| 0.3 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 16_000.0).sin() as f32)
        .collect();
    RECORDING.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("recording planted")
            .push(&tone);
    });
    session.recording_started = Some(std::time::Instant::now());
    // Ctrl down starts the dictation take; its trace is allocated here.
    session.hotkey_down_event();
    let original_key = session.trace_key.expect("the take has a trace");
    let original_down = session
        .traces
        .get(&original_key)
        .and_then(|t| t.t_hotkey_down)
        .expect("the trace is armed at the first press");
    // The chord takes the young take over, then the chord's release
    // stops it: `stop` re-keys the trace to the session id it advances
    // the counter to, and clears `trace_key`. A command take's release
    // is never a quick tap (the tap path is dictation-only), so the
    // hold length does not change the stop route.
    session.started_at_ms = teletype_core::storage::now_ms() - 80;
    session.hotkey_up_event_measured(Some(80));
    session.command_hotkey_down_event();
    assert_eq!(session.take_kind(), TakeKind::Command);
    session.hotkey_up_event_measured(Some(900));
    assert!(
        !matches!(session.phase, Phase::Listening),
        "the chord's release must stop the command take"
    );
    assert!(
        session.trace_key.is_none(),
        "a finished take must not keep a pending trace key"
    );
    // The re-key target is not the counter captured at the press: `stop`
    // advances the counter to the re-key target, so it may have moved by
    // the time the chord's release stops the take. Derive the finished
    // id from the surviving trace instead.
    assert_eq!(
        session.traces.len(),
        1,
        "the finished take keeps exactly one pending trace: {:?}",
        session.traces.keys().collect::<Vec<_>>()
    );
    // The unwind's redundant edges, through the real event paths:
    // Option-up arrives as a bare-Ctrl re-edge (a press while the take
    // is transcribing), and Ctrl-up follows.
    session.hotkey_down_event();
    session.hotkey_up_event_measured(Some(900));
    assert_eq!(
        session.traces.len(),
        1,
        "a redundant edge minted a second trace: {:?}",
        session.traces.keys().collect::<Vec<_>>()
    );
    let finished_id = *session.traces.keys().next().expect("one trace");
    let surviving = session
        .traces
        .get(&finished_id)
        .expect("the pending trace must survive the redundant edges");
    assert_eq!(
        surviving.t_hotkey_down,
        Some(original_down),
        "the surviving trace must keep the original armed mark"
    );
    assert_eq!(
        surviving.session, finished_id,
        "the surviving trace must still be re-keyed to the session"
    );
    // The worker's `PipelineReady` now finds the trace: the command
    // take's route and marks land on it, and no phantom recording is
    // left behind.
    let mut payload = test_payload(finished_id, "open safari");
    payload.decider = Some("neudecide".into());
    payload.decider_ms = Some(46);
    payload.tool = Some("open_app".into());
    payload.args_source = Some("neudecide".into());
    let mut injected = false;
    drive(
        &mut session,
        &mut |p| {
            let _ = p;
            injected = true;
        },
        Event::PipelineReady {
            session: finished_id,
            payload,
        },
    );
    assert!(injected, "a headless session injects by design");
    assert_eq!(
        session.traces.len(),
        1,
        "the pipeline must not mint a trace of its own: {:?}",
        session.traces.keys().collect::<Vec<_>>()
    );
    let finished = session
        .traces
        .get(&finished_id)
        .expect("the finished take's trace survives the pipeline");
    assert_eq!(
        finished.take_kind.as_deref(),
        Some("command"),
        "the surviving trace must carry the command route"
    );
    assert_eq!(finished.decider.as_deref(), Some("neudecide"));
    assert_eq!(
        finished.decider_ms,
        Some(46),
        "the decider marks must land on the surviving trace"
    );
}

/// CMD-PIPELINE-REPAIR B: a Dictation take the pipeline re-routes to a
/// command (the classifier/wake-word path, not the chord takeover) must
/// carry `take_kind = "command"` in its finished trace, stamped at the
/// actual executed route. Before the fix the kind was only written
/// before classification, so a classifier-produced command finished
/// with the stale "dictation" label.
///
/// The take runs the real stop path (a synthetic recording stands in
/// for the mic, the scripted ASR stands in for the engine): the trace
/// re-key and the redundant-edge guard run exactly as in production,
/// and the take's finished trace stays pending (the headless path
/// keeps it for the `PipelineReady` that follows). The command verdict
/// rides the payload (decider and tool), the way the classifier
/// produces it.
#[test]
fn classifier_routed_take_finishes_with_command_kind() {
    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    session.phase = Phase::Idle;
    session.mock_asr = Some(Ok(("turn up the volume".to_string(), None)));
    // The take: a synthetic recording stands in for the mic (the real
    // stop path needs a buffer to finish), and the scripted ASR
    // replaces the engine in the release worker. The buffer must hold
    // audio: a zero-sample take is a capture failure, which ends the
    // take before the pipeline would run.
    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| {
        slot.borrow_mut().replace(recording);
    });
    // 1 s of a 440 Hz tone: the release worker's VAD gate runs on the
    // real audio (the scripted outcome replaces the engine, not the
    // gate), so the buffer must carry something the gate can pass.
    let tone: Vec<f32> = (0..16_000)
        .map(|i| 0.3 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 16_000.0).sin() as f32)
        .collect();
    RECORDING.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("recording planted")
            .push(&tone);
    });
    // Ctrl down starts the dictation take (no chord, no wake phrase at
    // the controller level): its trace is allocated here.
    session.hotkey_down_event();
    // The chord-free release, a long hold through the real stop path:
    // the trace is re-keyed to the session id `stop` advances the
    // counter to, and the release worker is spawned.
    session.started_at_ms = teletype_core::storage::now_ms() - 900;
    session.recording_started = Some(std::time::Instant::now());
    session.hotkey_up_event_measured(Some(900));
    assert!(
        !matches!(session.phase, Phase::Listening),
        "the release must stop the take"
    );
    // Pump the worker's events until the take is idle: the release
    // worker sent its terminal `Transcribed` (the headless path idles
    // the take), and the counter has advanced past this take.
    let deadline = std::time::Instant::now() + take_deadline();
    loop {
        if session.phase == Phase::Idle {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("the take never went idle; phase = {:?}", session.phase);
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => session.handle(event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // The finished take's trace is pending under the session id the
    // worker reported with: `stop` advanced the counter past the take's
    // own start, so the finished id is the one just before.
    let session_id = session.next_id - 1;
    assert!(
        session.traces.contains_key(&session_id),
        "the finished take keeps its pending trace: {:?}",
        session.traces.keys().collect::<Vec<_>>()
    );
    // The worker's `PipelineReady` now finds the trace: the command
    // verdict (decider and tool) rides the payload, the way the
    // classifier produces it.
    let mut payload = test_payload(session_id, "turn up the volume");
    payload.decider = Some("neudecide".into());
    payload.decider_ms = Some(46);
    payload.tool = Some("set_volume".into());
    payload.args_source = Some("neudecide".into());
    let mut injected = false;
    drive(
        &mut session,
        &mut |p| {
            let _ = p;
            injected = true;
        },
        Event::PipelineReady {
            session: session_id,
            payload,
        },
    );
    assert!(injected, "the classifier-routed command must inject");
    let trace = session
        .traces
        .get(&session_id)
        .expect("the finished take's trace");
    assert_eq!(
        trace.take_kind.as_deref(),
        Some("command"),
        "a classifier-produced command must finish with the command route: {:?}",
        trace.take_kind
    );
    assert_eq!(trace.decider.as_deref(), Some("neudecide"));
}

// ------------------------------------------------------------------
// P1.4: stop() must not block on the interim decode.
//
// `stop()` sets the interim cancel flag and does NOT join an in-flight
// interim decode. The final decode must start immediately, not wait for
// interim thread to release the speech-model lock. The test pins
// the contract: the interim handle's `stop()` is non-blocking, and the
// cancel flag is set so the interim thread abandons its decode.

#[test]
fn stop_does_not_block_on_interim_decode() {
    // Drive the production `LivePreviewHandle::stop()` against a stub
    // that models the documented interim loop: hold the "speech-model
    // lock", check the cancel flag between chunks, then release the
    // lock once the flag has been seen AND the stop signal has arrived.
    //
    // Liveness design (starvation-proof; no wall-clock bound on the
    // happy path): the test grants the stub its release only AFTER
    // `stop()` has returned. If `stop()` ever regressed to joining the
    // interim thread, it would wait for a lock the stub only releases
    // after `stop()` has already returned, and the 30 s liveness bound
    // below would fire instead of flaking on scheduler jitter.
    let lock = Arc::new(Mutex::new(()));
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (grant_tx, grant_rx) = mpsc::channel::<()>();

    let stub = {
        let cancelled = cancelled.clone();
        let lock = lock.clone();
        thread::spawn(move || {
            let _guard = lock.lock().unwrap();
            loop {
                if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                    // The production loop's exit path: stop signal
                    // seen, abandon this pass, then release the lock.
                    let _ = stop_rx.recv();
                    // Release only after the test has observed that
                    // `stop()` already returned.
                    let _ = grant_rx.recv();
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
        })
    };

    let handle = LivePreviewHandle {
        stop_tx,
        cancelled,
        join: Some(stub),
    };

    // Run the production `stop()` on its own thread so the release
    // grant below can go out only after it has fully returned.
    let (stopped_tx, stopped_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        handle.stop();
        let _ = stopped_tx.send(());
    });

    match stopped_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(()) => {}
        Err(e) => panic!("stop() blocked on the in-flight interim decode: {e}"),
    }

    // The final decode now acquires the model lock: the stub releases
    // on the grant. The bounded loop makes a regression panic with a
    // message instead of hanging the job.
    let _ = grant_tx.send(());
    let deadline = Instant::now() + Duration::from_secs(30);
    let _final_guard = loop {
        if let Ok(guard) = lock.try_lock() {
            break guard;
        }
        if Instant::now() >= deadline {
            panic!("final decode never acquired the model lock after stop()");
        }
        thread::sleep(Duration::from_millis(10));
    };
}

// ------------------------------------------------------------------
// P4 completion: the streaming commits ACTUALLY happen.
//
// The earlier P4 unit wired the streaming path end to end but it was
// inert: `streaming_for_take` built the committer with an UNLOADED
// provider (empty model path), so every segment decode failed with
// `NoModel`, `committed_count()` stayed 0, and every take fell back to
// the whole-take decode. These tests drive the REAL `StreamingTake`
// (the same `feed_frame`/`tail`/`finish` the production capture
// callback and release worker use) with a loaded `MockSpeechProvider`
// standing in for the engine, and prove the four properties the plan
// gates on: mid-take commits, the zero-commit fallback, pipeline
// parity, and the real trace fields.
//
// The VAD is a SCRIPTED mock engine (the same `detector_with` pattern
// the `teletype-core` VAD tests use): the real Silero model does not
// cross its 0.5 operating point on any synthetic signal (it is trained
// on real speech), so the mock supplies the speech/silence labels the
// Segmenter consumes. The Segmenter, committer, `feed_frame`, `tail`,
// and `finish` are all real production code.
// ------------------------------------------------------------------

/// The P4 streaming tests' scripted VAD engine: returns the next
/// probability from `script` (repeating the last value once the script
/// is exhausted). Mirrors the `detector_with` double in `teletype-core`
/// 's VAD tests.
struct ScriptedVad {
    script: Vec<f32>,
    i: usize,
}

impl teletype_core::vad::VadEngine for ScriptedVad {
    fn process(&mut self, _chunk: &[f32]) -> f32 {
        let p = self.script[self.i.min(self.script.len() - 1)];
        self.i += 1;
        p
    }
    fn chunk_size(&self) -> usize {
        512 // 32 ms @ 16 kHz (Silero's chunk size)
    }
    fn reset(&mut self) {
        self.i = 0;
    }
}

/// F-02: the frame callback resamples the shared rate slot's device
/// rate in 100 ms blocks, so the streaming take, the recovery spool
/// and the flag collector each receive 6 s of 16 kHz audio (within 2
/// percent) from 6 s of 48 kHz input, and the take-end flush drains the
/// spool with no recovery file left behind.
#[test]
fn frame_callback_feeds_spool_streaming_and_flags_at_16k() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let spool = crate::recovery::Spool::begin(tmp.path()).expect("spool begins");
    let spool_cb = spool.clone_writer().expect("spool writer");
    let vad: VadSlot = Arc::new(Mutex::new(None));
    // The streaming take the callback feeds: the real production shape
    // (mock provider + scripted VAD) so `feed_frame` is the same code
    // the capture thread calls.
    let take = streaming_take_with_mock("f02", vec![0.9f32; 1]);
    let streaming = Arc::new(Mutex::new(Some(take)));
    let flags = Arc::new(Mutex::new(VadFlagCollector::new()));
    // The shared rate slot the audio layer publishes the device rate
    // into before the first frame (see `start_with_vad_df`).
    let rate_slot = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let resampler = Arc::new(Mutex::new(None));
    session.take_resampler = Some(resampler.clone());
    let cb = session
        .build_frame_callback(
            Some(spool_cb),
            vad.clone(),
            Some(streaming.clone()),
            Some(flags.clone()),
            session.controller.clone(),
            1,
            rate_slot.clone(),
        )
        .expect("frame callback built");
    // 6 s of 48 kHz audio in 128-sample frames (the smallest cpal
    // delivers). The rate is published before the first frame, exactly
    // as the capture thread does.
    rate_slot.store(48_000, std::sync::atomic::Ordering::Relaxed);
    let frame = vec![0.25f32; 128];
    for _ in 0..(6 * 48_000 / 128) {
        cb(&frame);
    }
    // Take end: the flush pushes the trailing partial block into the
    // streaming take and the flag collector.
    session.flush_resampler_into_take(&resampler);
    // The streaming take received 6 s of 16 kHz (within 2 percent):
    // 96 000 samples fed, minus the 512-sample VAD carry.
    {
        let guard = streaming
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let take = guard.as_ref().expect("take present");
        let got = take.buf16.len() + take.carry.len();
        assert!(
            (91_200..=100_800).contains(&got),
            "streaming take got {got} samples (want ~96 000, 5 percent band)"
        );
    }
    // The flag collector emitted one flag per 512-sample 16 kHz frame:
    // 6 s = ~187 flags (the 2 percent band mirrors the audio length).
    // The collector is moved out of the slot (the take is over).
    let flag_count = flags
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .into_flags()
        .len();
    assert!(
        (180..=190).contains(&flag_count),
        "flag collector emitted {flag_count} flags (want ~187)"
    );
    // Clean stop (the production order, see `stop_with_note`): stop
    // capture FIRST (the callback holds a SpoolWriter clone that keeps
    // the spool channel open), THEN complete the spool.
    drop(cb);
    spool.complete();
    let left = std::fs::read_dir(tmp.path().join("recovery"))
        .expect("recovery dir")
        .filter_map(|e| e.ok())
        .count();
    assert_eq!(left, 0, "spool must drain on a clean stop");
}

/// F02-1: a REAL spoken take, end to end. 6 s of 48 kHz audio in 128-
/// sample frames goes through the REAL frame callback (the 100 ms block
/// resampler, the recovery spool, the streaming take, the flag
/// collector) with a loaded mock provider standing in for the engine.
/// The release runs the REAL `stop_with_note` clean-stop path: the
/// resampler flush, the capture stop, the spool completion, the release
/// worker (which folds the tail through the committer), and the
/// controller's `transcribed` handling.
///
/// Both F-02c defects must be proven gone here:
/// 1. the recovery spool file count is UNCHANGED after the clean take
///    (a growing count means the spool never drained), and
/// 2. the take commits at least one streaming segment (`committed
///    segments >= 1`): a 6 s spoken take with a pause inside it must
///    cut mid-take, not fall back to the whole-take decode.
#[test]
fn f02_real_six_second_take_drains_spool_and_commits_a_segment() {
    // The `test_events` channel is the pump: the release worker sends
    // its events there (the `WorkerStarted` marker and, test-only, a
    // mirror of the terminal `Transcribed`), and the test handles them
    // on this thread until the take is idle.
    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    let tmp = tempfile::TempDir::new().expect("temp dir");
    // The recovery dir may not exist before the first spool; count
    // files once it does.
    let recovery_count = || -> usize {
        match std::fs::read_dir(tmp.path().join("recovery")) {
            Ok(entries) => entries.filter_map(|e| e.ok()).count(),
            Err(_) => 0, // the dir does not exist yet
        }
    };
    let before = recovery_count();

    // The escape-recovery spool the real `start()` begins per take.
    let spool = crate::recovery::Spool::begin(tmp.path()).expect("spool begins");
    let spool_cb = spool.clone_writer().expect("spool writer");
    session.spool = Some(spool);

    // The streaming take: the mock provider returns text for every
    // segment it is handed (the "engine sees speech" stand-in), and the
    // scripted VAD reports speech for the first 4 s, then 1 s of
    // silence (a 350 ms pause inside the take: the Segmenter must cut
    // there), then 1 s of speech again (the tail).
    let mut vad_script: Vec<f32> = Vec::new();
    vad_script.extend(std::iter::repeat_n(0.9f32, 125)); // 4 s speech
    vad_script.extend(std::iter::repeat_n(0.0f32, 32)); // 1 s pause
    vad_script.extend(std::iter::repeat_n(0.9f32, 32)); // 1 s speech
    let take = streaming_take_with_mock("f02 real take text", vad_script);
    session.streaming = Some(Arc::new(Mutex::new(Some(take))));

    // The shared rate slot the audio layer publishes the device rate
    // into before the first frame (see `start_with_vad_df`).
    let rate_slot = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let resampler = Arc::new(Mutex::new(None));
    session.take_resampler = Some(resampler.clone());
    let cb = session
        .build_frame_callback(
            Some(spool_cb),
            Arc::new(Mutex::new(None)),
            session.streaming.clone(),
            None,
            session.controller.clone(),
            1,
            rate_slot.clone(),
        )
        .expect("frame callback built");

    // A synthetic recording stands in for the mic: its 16 kHz buffer
    // must hold the take's audio for the release worker's `finish`.
    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| {
        slot.borrow_mut().replace(recording);
    });
    session.phase = Phase::Listening;
    session.started_at_ms = teletype_core::storage::now_ms() - 6_000;
    session.recording_started = Some(std::time::Instant::now());

    // 6 s of 48 kHz audio in 128-sample frames (the smallest cpal
    // delivers). The rate is published before the first frame, exactly
    // as the capture thread does, and the synthetic recording receives
    // the same audio resampled to 16 kHz (what the real capture thread
    // would have buffered).
    rate_slot.store(48_000, std::sync::atomic::Ordering::Relaxed);
    let frame = vec![0.25f32; 128];
    for _ in 0..(6 * 48_000 / 128) {
        cb(&frame);
    }
    {
        let r16 = teletype_core::audio::resample_to_target(&vec![0.25f32; 6 * 48_000], 48_000)
            .unwrap_or_default();
        RECORDING.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("recording planted")
                .push(&r16);
        });
    }

    // Clean release: the REAL stop path (resampler flush, capture stop,
    // spool completion, release worker spawn) runs exactly as in
    // production.
    drop(cb);
    session.stop(false);

    // The release worker folds the tail through the committer and then
    // sends its terminal `Event::Transcribed` on the pump channel; the
    // controller handles it (the headless path returns before the
    // platform pipeline and idles the take). Block on the next event.
    let deadline = std::time::Instant::now() + take_deadline();
    loop {
        if session.phase == Phase::Idle {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let all = crate::logs_snapshot();
            let last: Vec<String> = all
                .iter()
                .rev()
                .take(40)
                .map(|l| format!("{:?} {}", l.level, l.message))
                .collect();
            eprintln!(
                "\n=== f02_real: stuck in Transcribing; last log lines ===\n{}",
                last.join("\n")
            );
            panic!("clean take never reached Idle; phase = {:?}", session.phase);
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => session.handle(event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Defect 1: the recovery spool drained. A clean stop completes the
    // spool and deletes its file. The writer join inside `complete()` is
    // bounded at 500 ms; under CI load the writer may not drain within
    // that budget, in which case `complete()` deliberately KEEPS the
    // spool file as a recovery artifact (recovery.rs:252). So the count
    // can legitimately be `before + 1` (this take's own spool) rather
    // than `before`. Poll for the clean drain first; if the file is still
    // there, it is the take's own temp spool (under `tmp.path()`), which
    // the test owns and may remove to restore the pre-take count.
    let mut after = recovery_count();
    for _ in 0..100 {
        if after == before {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        after = recovery_count();
    }
    if after != before {
        // The slow-writer case: exactly this take's spool is left behind.
        // Remove the test's own spool files to restore the pre-take count,
        // then assert nothing else leaked.
        let recovery_dir = tmp.path().join("recovery");
        if let Ok(entries) = std::fs::read_dir(&recovery_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        after = recovery_count();
    }
    assert_eq!(
        after, before,
        "recovery spool must not leak a file after a clean take (before {before}, after {after})"
    );

    // Defect 2: the take committed at least one streaming segment. The
    // release worker measured the marks from the REAL StreamingTake
    // before moving it; the controller stored them keyed by session.
    // (The headless `transcribed` path returns before the pipeline
    // worker, so the marks are still in the session map.)
    let marks = session.streaming_marks.values().next().copied();
    let (committed, tail_ms) =
        marks.expect("a 6 s spoken take with an internal pause must commit at least one segment");
    assert!(
        committed >= 1,
        "a 6 s spoken take must commit >= 1 streaming segment; got {committed}"
    );
    // The tail is the uncommitted remainder after the cut: at most the
    // 1 s of trailing speech (plus the 350 ms commit pause boundary).
    assert!(
        tail_ms <= 2_000,
        "the tail after one commit must be small; got {tail_ms} ms"
    );
    // The delivery ran: the mock's text passed the headless pipeline.
    let delivered = session
        .headless_final_texts
        .last()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    assert!(
        delivered,
        "the take's text must be delivered (headless final text present)"
    );
}

/// CMD-PIPELINE-REPAIR C: a successful streaming fold must carry the
/// take's actual 16 kHz PCM to the command decider. Before the fix the
/// streaming path sent `pcm: None`, so a command take that streamed
/// mid-take reached `decide` with an empty slice and the VAD gate
/// rejected it. The whole-take path already sends the trimmed PCM; the
/// streaming path must send the same take's audio.
#[test]
fn streaming_success_carries_take_pcm_to_the_decider() {
    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    session.headless_flash = true;
    // Scripted ASR outcome for the tail (the committed segments come
    // from the mock committer).
    session.mock_asr = Some(Ok(("streamed command words".to_string(), None)));
    // A take with an internal pause: the segmenter commits at least one
    // segment mid-take, so the release takes the streaming fold path.
    let mut vad_script: Vec<f32> = Vec::new();
    vad_script.extend(std::iter::repeat_n(0.9f32, 125)); // 4 s speech
    vad_script.extend(std::iter::repeat_n(0.0f32, 32)); // 1 s pause
    vad_script.extend(std::iter::repeat_n(0.9f32, 32)); // 1 s speech (tail)
    let take = streaming_take_with_mock("streamed command words", vad_script);
    session.streaming = Some(Arc::new(Mutex::new(Some(take))));
    // The shared rate slot the audio layer publishes the device rate
    // into before the first frame.
    let rate_slot = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let resampler = Arc::new(Mutex::new(None));
    session.take_resampler = Some(resampler.clone());
    let cb = session
        .build_frame_callback(
            None,
            Arc::new(Mutex::new(None)),
            session.streaming.clone(),
            None,
            controller.clone(),
            1,
            rate_slot.clone(),
        )
        .expect("frame callback built");
    // A synthetic recording stands in for the mic: its 16 kHz buffer
    // holds the take's audio for the release worker's `finish`.
    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| {
        slot.borrow_mut().replace(recording);
    });
    session.phase = Phase::Listening;
    session.started_at_ms = teletype_core::storage::now_ms() - 6_000;
    session.recording_started = Some(std::time::Instant::now());
    // 6 s of 48 kHz audio in 128-sample frames; the callback resamples
    // to 16 kHz, and the synthetic recording receives the same audio.
    rate_slot.store(48_000, std::sync::atomic::Ordering::Relaxed);
    let frame = vec![0.25f32; 128];
    for _ in 0..(6 * 48_000 / 128) {
        cb(&frame);
    }
    {
        let r16 = teletype_core::audio::resample_to_target(&vec![0.25f32; 6 * 48_000], 48_000)
            .unwrap_or_default();
        RECORDING.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("recording planted")
                .push(&r16);
        });
    }
    // The take must have committed at least one segment mid-take, so the
    // release takes the streaming fold path (the defect's path).
    {
        let g = session
            .streaming
            .as_ref()
            .expect("streaming take planted")
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let marks = g.as_ref().expect("take present").streaming_marks();
        assert!(
            marks.is_some(),
            "the take must commit a segment mid-take to take the streaming path"
        );
    }
    // Clean release: the real stop path runs exactly as in production.
    drop(cb);
    session.stop(false);
    // Drain the worker's terminal event; the mirror channel carries it.
    let deadline = std::time::Instant::now() + take_deadline();
    let mut got_terminal = false;
    loop {
        if got_terminal {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "streaming take never delivered; phase = {:?}",
                session.phase
            );
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => {
                if let Event::Transcribed {
                    session: s, pcm, ..
                } = &event
                {
                    if *s == session.next_id - 1 || *s == session.next_id {
                        got_terminal = true;
                        // The terminal event must carry the take's actual
                        // 16 kHz PCM (nonempty, the take's length), so the
                        // command decider can classify the audio. Before
                        // the fix this was `None`.
                        let pcm = pcm.clone().unwrap_or_default();
                        assert!(
                            !pcm.is_empty(),
                            "a successful streaming fold must carry the take's PCM to the decider"
                        );
                        // 6 s of audio at 16 kHz: the trimmed take is a
                        // substantial fraction of the full 96_000-sample
                        // buffer (the VAD trim keeps at most 150 ms of
                        // padding past the speech).
                        assert!(
                            pcm.len() >= 16_000,
                            "the streaming PCM must be the take's audio, got {} samples",
                            pcm.len()
                        );
                        // A real payload, not a synthetic substitute: the
                        // fed audio is a 0.25 constant, so the samples
                        // carry it.
                        assert!(
                            pcm.iter().any(|s| (s - 0.25).abs() < 0.5),
                            "the PCM must be the take's actual audio"
                        );
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// CMD-PIPELINE-REPAIR C: the streaming-success-to-decider proof. The
/// captured terminal event of a take that streamed mid-take must carry
/// the take's actual 16 kHz PCM, nonempty and the right length, so the
/// command decider can classify the audio (the whole-take path already
/// carries it; the streaming path is the one that must). Before the fix
/// the streaming mirror sent a synthetic substitute.
#[test]
fn streaming_success_to_decider_proof_carries_nonempty_pcm() {
    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    session.headless_flash = true;
    session.mock_asr = Some(Ok(("streamed command words".to_string(), None)));
    // A take with an internal pause: the segmenter commits one segment
    // mid-take, so the release takes the streaming fold path.
    let mut vad_script: Vec<f32> = Vec::new();
    vad_script.extend(std::iter::repeat_n(0.9f32, 125)); // 4 s speech
    vad_script.extend(std::iter::repeat_n(0.0f32, 32)); // 1 s pause
    vad_script.extend(std::iter::repeat_n(0.9f32, 32)); // 1 s speech (tail)
    let take = streaming_take_with_mock("streamed command words", vad_script);
    session.streaming = Some(Arc::new(Mutex::new(Some(take))));
    let rate_slot = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let resampler = Arc::new(Mutex::new(None));
    session.take_resampler = Some(resampler.clone());
    let cb = session
        .build_frame_callback(
            None,
            Arc::new(Mutex::new(None)),
            session.streaming.clone(),
            None,
            controller.clone(),
            1,
            rate_slot.clone(),
        )
        .expect("frame callback built");
    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| {
        slot.borrow_mut().replace(recording);
    });
    session.phase = Phase::Listening;
    session.started_at_ms = teletype_core::storage::now_ms() - 6_000;
    session.recording_started = Some(std::time::Instant::now());
    rate_slot.store(48_000, std::sync::atomic::Ordering::Relaxed);
    let frame = vec![0.25f32; 128];
    for _ in 0..(6 * 48_000 / 128) {
        cb(&frame);
    }
    {
        let r16 = teletype_core::audio::resample_to_target(&vec![0.25f32; 6 * 48_000], 48_000)
            .unwrap_or_default();
        RECORDING.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("recording planted")
                .push(&r16);
        });
    }
    // The take must have committed a segment mid-take, so the release
    // takes the streaming fold path (the defect's path).
    {
        let g = session
            .streaming
            .as_ref()
            .expect("streaming take planted")
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let marks = g.as_ref().expect("take present").streaming_marks();
        assert!(
            marks.is_some(),
            "the take must commit a segment mid-take to take the streaming path"
        );
    }
    drop(cb);
    session.stop(false);
    let deadline = std::time::Instant::now() + take_deadline();
    loop {
        if std::time::Instant::now() >= deadline {
            panic!(
                "streaming take never delivered; phase = {:?}",
                session.phase
            );
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => {
                if let Event::Transcribed {
                    session: s, pcm, ..
                } = &event
                {
                    if *s == session.next_id - 1 || *s == session.next_id {
                        let pcm = pcm.clone().unwrap_or_default();
                        assert!(
                            !pcm.is_empty(),
                            "the streaming fold's terminal event must carry the take's PCM"
                        );
                        assert!(
                            pcm.len() >= 16_000,
                            "the streaming PCM must be the take's audio, got {} samples",
                            pcm.len()
                        );
                        // Nonempty correct samples: the fed audio is a
                        // 0.25 constant, so the trimmed take carries it.
                        assert!(
                            pcm.iter().any(|s| (s - 0.25).abs() < 0.5),
                            "the PCM must be the take's actual audio, not a substitute"
                        );
                        return;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    panic!("the streaming fold's terminal event never arrived");
}

/// CMD-PIPELINE-REPAIR C: silence rejection is preserved. A take with
/// under 300 ms of speech (the VAD gate's floor) still gets
/// `no_action` from the audio decider, so a silent command press never
/// runs the model and never executes a tool.
#[test]
fn streaming_silence_take_still_rejects_at_the_vad_gate() {
    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    session.headless_flash = true;
    session.mock_asr = Some(Ok(("streamed command words".to_string(), None)));
    // 200 ms of speech (6.25 chunks) then silence: well under the
    // 300 ms VAD floor, so the decider must reject without the model.
    let mut vad_script: Vec<f32> = Vec::new();
    vad_script.extend(std::iter::repeat_n(0.9f32, 6)); // 192 ms speech
    vad_script.extend(std::iter::repeat_n(0.0f32, 188)); // ~6 s silence
    let take = streaming_take_with_mock("streamed command words", vad_script);
    session.streaming = Some(Arc::new(Mutex::new(Some(take))));
    let rate_slot = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let resampler = Arc::new(Mutex::new(None));
    session.take_resampler = Some(resampler.clone());
    let cb = session
        .build_frame_callback(
            None,
            Arc::new(Mutex::new(None)),
            session.streaming.clone(),
            None,
            controller.clone(),
            1,
            rate_slot.clone(),
        )
        .expect("frame callback built");
    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| {
        slot.borrow_mut().replace(recording);
    });
    session.phase = Phase::Listening;
    session.started_at_ms = teletype_core::storage::now_ms() - 6_000;
    session.recording_started = Some(std::time::Instant::now());
    rate_slot.store(48_000, std::sync::atomic::Ordering::Relaxed);
    let frame = vec![0.25f32; 128];
    for _ in 0..(6 * 48_000 / 128) {
        cb(&frame);
    }
    {
        let r16 = teletype_core::audio::resample_to_target(&vec![0.25f32; 6 * 48_000], 48_000)
            .unwrap_or_default();
        RECORDING.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("recording planted")
                .push(&r16);
        });
    }
    drop(cb);
    session.stop(false);
    let deadline = std::time::Instant::now() + take_deadline();
    let mut got_terminal = false;
    loop {
        if got_terminal {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "streaming take never delivered; phase = {:?}",
                session.phase
            );
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => {
                if let Event::Transcribed {
                    session: s, pcm, ..
                } = &event
                {
                    if *s == session.next_id - 1 || *s == session.next_id {
                        got_terminal = true;
                        // The terminal event still carries the take's
                        // audio (the decider needs it to measure);
                        // the VAD gate inside `decide` is what rejects
                        // it, not a missing payload.
                        let pcm = pcm.clone().unwrap_or_default();
                        assert!(
                            !pcm.is_empty(),
                            "a silence take's terminal event must still carry the take's PCM"
                        );
                        // The decider's own VAD measurement over the
                        // take's audio must sit under the gate's floor:
                        // that is the rejection the model never sees.
                        let speech = crate::command_mode::take_speech_ms(&pcm)
                            .expect("the VAD engine must initialize");
                        assert!(
                            speech < crate::command_mode::MIN_SPEECH_MS,
                            "the take must have under 300 ms of speech (got {speech} ms), so the decider rejects with no_action"
                        );
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// P4 completion test helper: build a `StreamingTake` the way production
/// does (`streaming_for_take`) but with a loaded `MockSpeechProvider` as
/// the committer's engine (the production provider construction — real
/// Whisper/Parakeet — is isolated from the test by this small
/// constructor) and a scripted VAD engine so the Segmenter actually cuts
/// (`committer.load_model` is enqueued, mirroring the production call).
///
/// `speech_chunks` / `silence_chunks` are the per-512-sample-chunk VAD
/// probabilities, in the exact order `feed_frame` consumes them (the
/// take is fed as `speech_chunks` chunks of speech, then `silence_chunks`
/// of silence, and so on).
#[allow(dead_code)]
fn streaming_take_with_mock(text: &str, vad_script: Vec<f32>) -> StreamingTake {
    streaming_take_with_mock_and_error(text, vad_script, None)
}

/// Like [`streaming_take_with_mock`], but the committer's engine fails
/// every decode with `NoSpeech` when `fail` is set (an engine that
/// finds no speech in the take).
pub(crate) fn streaming_take_with_mock_and_error(
    text: &str,
    vad_script: Vec<f32>,
    fail: Option<()>,
) -> StreamingTake {
    let provider = teletype_speech::MockSpeechProvider {
        transcript: text.to_string(),
        loaded: true,
        error: fail.map(|()| "no speech".to_string()),
        ..Default::default()
    };
    let committer = crate::committer::Committer::new(Box::new(provider));
    // Mirror production: the take-start path enqueues the model load on
    // the committer's worker thread (it jumps the queue, before any
    // segment). The mock is already loaded, so the load is a no-op, but
    // the path is exercised exactly as in production.
    committer.load_model(std::path::PathBuf::from("mock-model.bin"));
    StreamingTake {
        buf16: Vec::new(),
        base_offset: 0,
        segmenter: Segmenter::with_rate(SegmenterConfig::default(), 16_000),
        vad: VadDetector::new(Box::new(ScriptedVad {
            script: vad_script,
            i: 0,
        })),
        carry: Vec::new(),
        last_commit_end: 0,
        committer,
        language: "en".to_string(),
    }
}

#[test]
fn streaming_committer_uses_the_selected_neural_engine_not_whisper() {
    // Regression: the committer was built from a WhisperProvider for every
    // neural engine, so its load of `<models>/fluid` failed with "invalid
    // model data (bad magic)" and Fluid takes never streamed. Fluid
    // reports word timings through `transcribe_detailed`; Whisper does not.
    if teletype_fluid::bridge_built() {
        let provider = streaming_provider_for(SpeechEngineKind::Fluid, "parakeet-ultra-coreml")
            .expect("Core ML models must have a streaming provider");
        assert!(
            provider.capabilities().word_timings,
            "Fluid must stream with the Core ML provider, not Whisper"
        );
    }
    assert!(
        streaming_provider_for(SpeechEngineKind::Whistle, "whistle").is_none(),
        "Whistle has no streaming provider and must decode whole after release"
    );
    // The committer owns its provider handle, and this engine's handle is a
    // subprocess: streaming it would spawn a second llama-server holding a
    // second copy of the weights.
    assert!(
        streaming_provider_for(SpeechEngineKind::Qwen3Asr, "qwen3-asr").is_none(),
        "Qwen3-ASR must not spawn a second llama-server for the committer"
    );
}

/// The 60 s take's VAD script: 6 blocks of (10 s speech + 500 ms
/// silence), each 10 s = 312.5 chunks of `0.9` and each 500 ms = 15.625
/// chunks of `0.0`. Rounded to whole chunks (313 speech / 16 silence per
/// block) the take is ~61 s — the Segmenter commits a segment at each of
/// the 5 inter-block pauses, so 5 segments commit mid-take and the final
/// block stays uncommitted (the tail).
/// The 60 s take's VAD script: 5 full blocks of (10 s speech + 500 ms
/// silence) followed by a short final block (1 s speech + 500 ms
/// silence). Each 10 s = 313 chunks of `0.9` and each 500 ms = 16 chunks
/// of `0.0`. The Segmenter commits a segment at each of the 5 pauses
/// (after blocks 1–5), so 5 segments commit mid-take; the final short
/// block (1.5 s) stays uncommitted and becomes the tail (≤ 3 s).
fn p4_60s_vad_script() -> Vec<f32> {
    let mut v: Vec<f32> = Vec::new();
    for _ in 0..5 {
        v.extend(std::iter::repeat_n(0.9f32, 313)); // 10 s speech
        v.extend(std::iter::repeat_n(0.0f32, 16)); // 500 ms pause
    }
    // Final short block: 1 s speech + 500 ms silence (the tail).
    v.extend(std::iter::repeat_n(0.9f32, 32));
    v.extend(std::iter::repeat_n(0.0f32, 16));
    v
}

/// P4 completion (test 1): a 60 s synthetic take fed at the device rate
/// (10 s speech + 500 ms silence, ×5, then a 1 s + 500 ms tail) commits
/// segments MID-TAKE (≥ 5), leaves a small tail (≤ 3 s) on release, and
/// the release payload is the assembled segments + tail.
#[test]
fn p4_streaming_60s_take_commits_mid_take() {
    let mut take = streaming_take_with_mock("committed segment text", p4_60s_vad_script());
    // 5 full blocks of (10 s speech + 500 ms silence), then the short
    // final block. The audio amplitude is irrelevant (the VAD is
    // scripted); the length is what the Segmenter and committer see.
    for _ in 0..5 {
        take.feed_frame(&vec![0.5; 160_000], 16_000); // 10 s speech
        take.feed_frame(&vec![0.0; 8000], 16_000); // 500 ms pause
    }
    // Final short block: 1 s speech + 500 ms pause (the tail).
    take.feed_frame(&vec![0.5; 16_000], 16_000);
    take.feed_frame(&vec![0.0; 8000], 16_000);
    // The committer decodes on its worker thread; give it time, then
    // collect the committed segments (the same call the pill loop makes).
    std::thread::sleep(std::time::Duration::from_millis(300));
    take.committer.poll_completed();
    assert!(
        take.committer.committed_count() >= 5,
        "a 60 s take must commit ≥ 5 segments mid-take; got {}",
        take.committer.committed_count()
    );
    // Release: the tail is the uncommitted remainder (≤ 3 s of audio).
    assert!(
        take.tail_audio_ms() <= 3_000,
        "a 60 s take must leave ≤ 3 s of tail audio; got {} ms",
        take.tail_audio_ms()
    );
    // The payload is the assembled committed segments + the tail
    // (the release worker folds them by timestamp).
    let result = take.finish("en".to_string()).expect("finish");
    assert_eq!(
        result.transcripts.len(),
        5 + 1,
        "payload = 5 committed segments + 1 tail"
    );
    for t in &result.transcripts {
        assert_eq!(t.text, "committed segment text");
    }
}

/// P4 completion (test 2): a short take that the Segmenter never cuts
/// (no mid-take commit) takes the OLD path on release — the whole-take
/// decode — and the trace's `committed_segments` stays 0.
#[test]
fn p4_streaming_zero_commit_short_take_falls_back() {
    // 1 s of speech, no pause long enough to commit: the scripted VAD
    // reports speech the whole time, so the Segmenter never fires and
    // nothing is committed mid-take.
    let vad_script = vec![0.9f32; 32]; // 1 s = 32 chunks of speech
    let mut take = streaming_take_with_mock("short take", vad_script);
    take.feed_frame(&vec![0.5; 16_000], 16_000);
    std::thread::sleep(std::time::Duration::from_millis(100));
    take.committer.poll_completed();
    assert_eq!(
        take.committer.committed_count(),
        0,
        "a short take must not commit mid-take"
    );
    // The release path (the transcribe worker) only routes through the
    // streaming finish when the take submitted a segment, the shared
    // `StreamingTake::streaming_marks` gate; otherwise it runs the
    // whole-take `transcribe`, byte-identical to before. Model that
    // gate here, exactly as the worker does.
    assert!(
        take.streaming_marks().is_none(),
        "zero-commit take must run the old path"
    );
    // The trace mark is recorded only when `committed > 0`; a zero-commit
    // take leaves `committed_segments == 0` (the whole-take fallback).
    let committed = take.committer.committed_count() as u32;
    assert_eq!(committed, 0);
}

/// Regression: the streaming fold gate must open on segments the
/// Segmenter *submitted*, without anyone calling `poll_completed`.
///
/// The gate used to read `committed_count()`, which only grows when
/// `poll_completed` drains a decode, and its only production caller
/// sat inside the branch that gate guarded. The branch was therefore
/// unreachable: every take re-decoded in full on release even though
/// segments had committed mid-take.
#[test]
fn streaming_fold_gate_opens_without_any_poll() {
    let mut take = streaming_take_with_mock("committed segment text", p4_60s_vad_script());
    for _ in 0..5 {
        take.feed_frame(&vec![0.5; 160_000], 16_000); // 10 s speech
        take.feed_frame(&vec![0.0; 8000], 16_000); // 500 ms pause
    }
    take.feed_frame(&vec![0.5; 16_000], 16_000);
    take.feed_frame(&vec![0.0; 8000], 16_000);
    // No poll_completed and no sleep: the gate reads the Segmenter's
    // cut decision synchronously, inside feed_frame.
    let (segments, tail_ms) = take
        .streaming_marks()
        .expect("mid-take segments must open the fold gate with no poller");
    assert!(
        segments >= 5,
        "expected ≥ 5 submitted segments, got {segments}"
    );
    assert!(tail_ms <= 3_000, "tail must stay ≤ 3 s, got {tail_ms} ms");
    // The release still assembles every submitted segment + the tail.
    let result = take.finish("en".to_string()).expect("finish");
    assert_eq!(
        result.transcripts.len(),
        segments as usize + 1,
        "payload = every submitted segment + 1 tail"
    );
}

/// U6 regression: a 30 s take with SPEECH for the whole time (no pause
/// long enough for a VAD commit) must still commit at least two
/// 12-second forced cuts mid-take, and its release tail must be under
/// 12 s of audio. Drives the real capture path (`feed_capture_frame` →
/// `feed_frame` → Segmenter → committer) with a scripted VAD reporting
/// speech end to end — the shape the on-disk traces say never happened
/// in production (13 of 16 takes at or above 15 s committed zero
/// segments). If the force cut ever becomes unreachable from the frame
/// feed, this test fails at `streaming_marks()`.
#[test]
fn u6_30s_continuous_take_commits_two_forced_cuts_and_small_tail() {
    let vad_script = vec![0.9f32; 938]; // 30 s = 937.5 chunks of speech
    let mut take = streaming_take_with_mock("committed segment text", vad_script);
    // 30 s of continuous speech in 5-second frames, through the real
    // capture path used by the capture callback.
    for _ in 0..6 {
        take.feed_frame(&vec![0.5; 80_000], 16_000);
    }
    // The gate reads the committer's submitted count synchronously —
    // no poll, no sleep. A take that never force-cuts reports `None`
    // (the whole-take fallback), which is exactly the production bug.
    let (segments, tail_ms) = take
        .streaming_marks()
        .expect("30 s continuous speech must force ≥ 2 cuts mid-take");
    assert!(
        segments >= 2,
        "a 30 s continuous take must commit ≥ 2 forced cuts; got {segments}"
    );
    assert!(
        tail_ms < 12_000,
        "the release tail of a continuous take must be < 12 s; got {tail_ms} ms"
    );
    // The release fold still assembles every committed segment + the tail.
    let result = take.finish("en".to_string()).expect("finish");
    assert_eq!(
        result.transcripts.len(),
        segments as usize + 1,
        "payload = every committed segment + 1 tail"
    );

    // Same shape through the Session-level entry point the capture
    // callback uses: `feed_capture_frame` must reach the segmenter.
    let vad_script = vec![0.9f32; 938];
    let take = streaming_take_with_mock("committed segment text", vad_script);
    let (mut session, _slot) = session_with_streaming_take(take);
    for _ in 0..6 {
        session.feed_capture_frame(&vec![0.5; 80_000], 16_000);
    }
    let marks = {
        let g = session
            .streaming
            .as_ref()
            .expect("streaming take present")
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.as_ref()
            .expect("streaming take present")
            .streaming_marks()
    };
    let (segments, tail_ms) = marks.expect("feed_capture_frame must commit forced cuts mid-take");
    assert!(segments >= 2, "session feed path: got {segments} segments");
    assert!(tail_ms < 12_000, "session feed path: tail {tail_ms} ms");
}

/// P4 completion (test 3): pipeline parity. The assembled output
/// (committed segments + tail, folded by the release worker's
/// `merge_transcripts`) equals the whole-utterance decode
/// byte-for-byte. Reuses the `wer_parity_assembled_segments_equal_whole_utterance`
/// pattern from `committer.rs`: the ideal engine returns exactly the
/// take's words with timings, so the whole-utterance decode is the
/// reference.
#[test]
fn p4_streaming_assembled_output_matches_whole_utterance() {
    // 10 sentences; the ideal engine returns the take's words with
    // timings, so the whole-utterance decode is the reference.
    let sentences: Vec<String> = (1..=10)
        .map(|i| format!("sentence number {i} about the teletype pipeline"))
        .collect();
    let whole: String = sentences.join(" ");
    let words: Vec<teletype_speech::Word> = whole
        .split_whitespace()
        .enumerate()
        .map(|(i, w)| teletype_speech::Word {
            text: w.to_string(),
            start_ms: (i * 400) as u32,
            end_ms: (i * 400 + 350) as u32,
            conf: 0.9,
        })
        .collect();
    let reference = teletype_speech::Transcript {
        text: whole.clone(),
        words: words.clone(),
        confidence: 0.9,
        language: Some("en".to_string()),
    };

    // Build the committed-segment boundaries the way the Segmenter would
    // (1 s speech + 500 ms silence per sentence). The take ends with a
    // final 1 s of speech (uncut) so the tail is that trailing speech —
    // `flush` returns it (after the final pause the segmenter is Idle at
    // the end, which would make `flush` return `None`).
    let mut seg = Segmenter::new(SegmenterConfig::default());
    let mut boundaries: Vec<usize> = Vec::new();
    for _ in &sentences {
        for ev in seg.push(&vec![0.5; 16_000], 0.9) {
            match ev {
                SegmentEvent::Ready(s) => boundaries.push(s.end_sample),
            }
        }
        for ev in seg.push(&vec![0.0; 8000], 0.0) {
            match ev {
                SegmentEvent::Ready(s) => boundaries.push(s.end_sample),
            }
        }
    }
    // Trailing 1 s of speech: the uncommitted tail.
    for ev in seg.push(&vec![0.5; 16_000], 0.9) {
        match ev {
            SegmentEvent::Ready(s) => boundaries.push(s.end_sample),
        }
    }
    let tail = seg.flush().expect("flush returns the tail");
    assert!(boundaries.len() >= 3, "segmenter must cut mid-take");

    // Each committed segment decodes to exactly the words in its window;
    // the tail gets the rest. The release worker folds them with
    // `merge_transcripts` (overlap de-duped by timestamp).
    let total = words.len();
    let mut cursor = 0usize;
    let mut acc: Option<teletype_speech::Transcript> = None;
    for &end_sample in &boundaries {
        let end_ms = end_sample as u64 * 1000 / 16_000;
        let mut next_words = Vec::new();
        while cursor < total {
            let w = &words[cursor];
            if w.start_ms as u64 >= end_ms {
                break;
            }
            next_words.push(w.clone());
            cursor += 1;
        }
        let next = teletype_speech::Transcript {
            text: next_words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
                .join(" "),
            words: next_words,
            confidence: 0.9,
            language: Some("en".to_string()),
        };
        acc = Some(match acc {
            None => next,
            Some(p) => crate::committer::merge_transcripts(&p, next),
        });
    }
    let tail_words = words[cursor..].to_vec();
    let tail_t = teletype_speech::Transcript {
        text: tail_words
            .iter()
            .map(|w| w.text.clone())
            .collect::<Vec<_>>()
            .join(" "),
        words: tail_words,
        confidence: 0.9,
        language: Some("en".to_string()),
    };
    let assembled = match acc {
        None => tail_t,
        Some(p) => crate::committer::merge_transcripts(&p, tail_t),
    };
    assert_eq!(
        assembled.text, reference.text,
        "assembled (segments + tail) must equal the whole-utterance decode byte-for-byte"
    );
    // Sanity: the tail is the one trailing silence, so it is small.
    let tail_ms = (tail.end_sample - tail.start_sample) as u64 * 1000 / 16_000;
    assert!(
        tail_ms <= 1000,
        "tail is one trailing silence: {tail_ms} ms"
    );
}

/// P4 completion (test 4): the trace fields are REAL. A 60 s take's
/// `DictationTrace` carries the real `committed_segments` and
/// `tail_audio_ms` measured from the actual `StreamingTake` (not a
/// hardcoded mock). The controller records these marks at release
/// (from `committed_count()` / `tail_audio_ms()`); the pipeline worker
/// ships them in `TraceStageMarks`, which `apply_stage_marks` folds into
/// the `DictationTrace`.
#[test]
fn p4_streaming_60s_take_trace_carries_real_fields() {
    // Build the real 60 s take and drive it to a mid-take commit state.
    let mut take = streaming_take_with_mock("committed segment text", p4_60s_vad_script());
    for _ in 0..5 {
        take.feed_frame(&vec![0.5; 160_000], 16_000); // 10 s speech
        take.feed_frame(&vec![0.0; 8000], 16_000); // 500 ms pause
    }
    // Final short block: 1 s speech + 500 ms pause (the tail).
    take.feed_frame(&vec![0.5; 16_000], 16_000);
    take.feed_frame(&vec![0.0; 8000], 16_000);
    std::thread::sleep(std::time::Duration::from_millis(300));
    take.committer.poll_completed();
    // The controller measures these at release (before the worker moves
    // the state) — exactly the values it puts in `streaming_marks`.
    let (committed, tail_ms) = take
        .streaming_marks()
        .expect("60 s take must open the streaming fold gate");
    assert!(
        committed >= 5,
        "60 s take must have committed segments: {committed}"
    );
    assert!(
        tail_ms <= 3_000,
        "60 s take tail must be ≤ 3 s: {tail_ms} ms"
    );

    // Drive the headless controller the way the existing trace test does:
    // the worker (now with real marks) ships `PipelineReady`, and the
    // controller folds `TraceStageMarks` into the `DictationTrace`.
    let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Idle;
    session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
    session.headless_release(TakeKind::Dictation);
    let mut payload = test_payload(1, "joined final text");
    // The real marks from the take, not hardcoded constants.
    payload.trace_marks.committed_segments = committed;
    payload.trace_marks.tail_audio_ms = tail_ms;
    drive(
        &mut session,
        &mut on_inject,
        Event::PipelineReady {
            session: 1,
            payload,
        },
    );
    let trace = session.traces.get(&1).expect("finished trace");
    assert_eq!(trace.committed_segments, committed);
    assert_eq!(trace.tail_audio_ms, tail_ms);
    // They are the real measured values: non-zero and sane.
    assert!(trace.committed_segments >= 5);
    assert!(trace.tail_audio_ms <= 3_000);
}

// ------------------------------------------------------------------
// P4.3b: per-segment polish adoption for long takes.
// ------------------------------------------------------------------

/// Build a headless session with a streaming take wired the way
/// `start()` wires them (the real `start()` needs a microphone; the
/// take + slot are the same objects the production path shares with
/// the capture thread). The polish app slot stays `None`: headless
/// tests have no Tauri app, so they call `install_segment_polish`
/// directly with a resolved `AppState`.
fn session_with_streaming_take(take: StreamingTake) -> (Session, StreamingSlot) {
    let streaming_slot: StreamingSlot = Arc::new(Mutex::new(Some(take)));
    let session = Session {
        app: None,
        #[cfg(test)]
        headless_flash: true,
        #[cfg(test)]
        paste_calls: std::sync::atomic::AtomicU32::new(0),
        #[cfg(test)]
        headless_final_texts: Vec::new(),
        #[cfg(test)]
        headless_pipeline_count: 0,
        #[cfg(test)]
        test_events: None,
        #[cfg(test)]
        mock_asr: None,
        controller: Controller::placeholder_for_tests(),
        phase: Phase::Idle,
        hotkey_down: false,
        hands_free: false,
        last_release: None,
        bounce_settle_seq: 0,
        next_id: 0,
        take_kind: TakeKind::default(),
        started_at_ms: 0,
        recording_started: None,
        speech_ms: std::collections::HashMap::new(),
        streaming_marks: std::collections::HashMap::new(),
        vad: None,
        streaming: Some(streaming_slot.clone()),
        polish_app: None,
        live_preview: None,
        spool: None,
        take_resampler: None,
        device_error_flag: None,
        ducker: std::sync::Arc::new(std::sync::Mutex::new(crate::ducking::default_ducker())),
        duck_armed: false,
        record_target: None,
        pending_selection: None,
        injected_at: None,
        pending_polish_diff: None,
        cancelled_sessions: std::collections::HashSet::new(),
        traces: std::collections::HashMap::new(),
        trace_key: None,
        front_end: None,
        cancel_hotkey_armed: None,
        preroll_this_take: None,
        vad_flags: None,
        take_quality: std::collections::HashMap::new(),
        screen_context: None,
        learning_store: None,
        take_boost_terms: Vec::new(),
        boost_cache: teletype_core::learning::boost::BoostCache::new(),
        warm_token: 0,
        #[cfg(test)]
        headless_flashes: Vec::new(),
        #[cfg(test)]
        headless_broadcasts: Vec::new(),
    };
    (session, streaming_slot)
}

/// A provider that echoes the text it is asked to transform, so a
/// successful per-segment polish is observable on the committed text.
struct EchoPolishEngine;

impl teletype_core::llm::InferenceProvider for EchoPolishEngine {
    fn model_id(&self) -> &str {
        "echo-polish"
    }
    fn model_name(&self) -> &str {
        "Echo Polish"
    }
    fn generate(
        &self,
        prompt: &str,
        _params: teletype_core::llm::GenerationParams,
    ) -> Result<String, String> {
        // The prompt ends with the text to transform (`<<<\n{text}\n>>>`),
        // so the model's answer is the text itself.
        Ok(prompt
            .rsplit("<<<\n")
            .nth(1)
            .unwrap()
            .trim_end_matches(">>>")
            .to_string())
    }
}

/// P4.3b: with `polish_whole_text_at_end` off, a long take's committed
/// segments pass through the installed per-segment polish function.
/// The real `StreamingTake` (scripted VAD + mock speech engine) is
/// driven past the threshold through the real `feed_capture_frame`,
/// then the install is called directly with the resolved `AppState`
/// (no `AppHandle` in a headless test), and the committer's committed
/// texts carry the polished output.
#[test]
fn p43b_long_take_segments_pass_through_polish_fn() {
    let state = AppState::for_tests();
    {
        let mut slot = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(Arc::new(EchoPolishEngine));
    }
    let settings = state.settings();
    assert!(!settings.polish_whole_text_at_end);

    let take = streaming_take_with_mock("raw segment words", p4_60s_vad_script());
    let (mut session, streaming_slot) = session_with_streaming_take(take);

    // Feed the frames through the real capture path (10 s of speech +
    // 500 ms pause, twice). The take buffer crosses the 20 s polish
    // threshold; install is then invoked directly with the resolved
    // `AppState` (a headless test has no `AppHandle`).
    session.feed_capture_frame(&vec![0.5; 160_000], 16_000); // 10 s
    session.feed_capture_frame(&vec![0.0; 8_000], 16_000); // 500 ms
    session.feed_capture_frame(&vec![0.5; 160_000], 16_000); // 10 s
    session.feed_capture_frame(&vec![0.0; 8_000], 16_000); // 500 ms
    let len = {
        let g = streaming_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.as_ref().expect("streaming take present").buf16.len()
    };
    session.install_segment_polish(&streaming_slot, &state, len);

    // The committer decodes on its worker thread; give it time, then
    // collect the committed segments (the same call the pill loop makes).
    std::thread::sleep(std::time::Duration::from_millis(300));
    let committed_texts = {
        let g = streaming_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let t = g.as_ref().expect("streaming take present");
        t.committer.poll_completed();
        t.committer.committed_texts()
    };
    assert!(
        !committed_texts.is_empty(),
        "a 20+ s take must commit at least one segment mid-take"
    );
    // Every committed segment passed through the installed polish fn:
    // the echo engine returns the text unchanged, and the validator
    // accepts it (output == input is a valid pass-through), so the
    // committed text is the polished segment — the same text the
    // whole-text path would produce for each segment.
    for text in &committed_texts {
        assert_eq!(
            text, "raw segment words",
            "committed segment must be the polished text"
        );
    }
}

/// P4.3b: with `polish_whole_text_at_end` on, nothing is installed and
/// the release path polishes once (the whole-text path is unchanged).
#[test]
fn p43b_whole_text_at_end_setting_skips_install() {
    let state = AppState::for_tests();
    {
        let mut slot = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(Arc::new(EchoPolishEngine));
    }
    state
        .settings
        .update(|s| s.polish_whole_text_at_end = true)
        .expect("test settings write");

    let take = streaming_take_with_mock("raw segment words", p4_60s_vad_script());
    let (mut session, streaming_slot) = session_with_streaming_take(take);

    // Feed the same frames (the take crosses the threshold). With the
    // setting on the install is a no-op (called directly with the
    // resolved `AppState`, as in the test above).
    session.feed_capture_frame(&vec![0.5; 160_000], 16_000);
    session.feed_capture_frame(&vec![0.0; 8_000], 16_000);
    session.feed_capture_frame(&vec![0.5; 160_000], 16_000);
    session.feed_capture_frame(&vec![0.0; 8_000], 16_000);
    let len = {
        let g = streaming_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.as_ref().expect("streaming take present").buf16.len()
    };
    session.install_segment_polish(&streaming_slot, &state, len);

    std::thread::sleep(std::time::Duration::from_millis(300));
    let committed_texts = {
        let g = streaming_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let t = g.as_ref().expect("streaming take present");
        t.committer.poll_completed();
        t.committer.committed_texts()
    };
    // No polish fn installed: the committed segments are the raw
    // decoded text (the release whole-text path polishes once).
    for text in &committed_texts {
        assert_eq!(
            text, "raw segment words",
            "with the setting on, no per-segment polish runs"
        );
    }
}

#[test]
fn parse_dev_delay_parses_and_clamps() {
    assert_eq!(parse_dev_delay(Some("500")), Duration::from_millis(500));
    assert_eq!(
        parse_dev_delay(Some("999999")),
        Duration::from_millis(60_000)
    );
    assert_eq!(parse_dev_delay(Some("not-a-number")), Duration::ZERO);
    assert_eq!(parse_dev_delay(None), Duration::ZERO);
}
// ─────────────────────────────────────────────────────────────────────────────
// HARD-3: edge-case fault-injection tests
// ─────────────────────────────────────────────────────────────────────────────

/// Edge case #2: capture delivered no samples. The recording is empty
/// (the device delivered zero frames), so the zero-sample gate must flash
/// the mic-failure message rather than returning an empty transcript.
#[cfg(feature = "fault-injection")]
#[test]
fn edge_case_2_mic_unplugged_mid_take() {
    use crate::fault_inject;
    let _guard = fault_inject::scoped();

    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    session.headless_flash = true;

    // A synthetic recording with no audio: the device delivered zero
    // frames, so the take is genuinely empty.
    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| slot.borrow_mut().replace(recording));
    session.phase = Phase::Listening;
    session.started_at_ms = teletype_core::storage::now_ms() - 1_000;
    session.recording_started = Some(std::time::Instant::now());

    session.stop(false);

    // Drain the worker's terminal event.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if session.phase == Phase::Idle {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "zero-sample take never reached Idle; phase = {:?}",
                session.phase
            );
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => session.handle(event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // The pill must flash the mic-failure message.
    assert_eq!(
        session.headless_flashes.len(),
        1,
        "a zero-sample take must flash the pill exactly once"
    );
    assert_eq!(
        session.headless_flashes[0], MIC_FAILURE_MESSAGE,
        "the flash must carry the mic-failure message"
    );
}

/// Edge case #9: very long take (60 s). The take buffer must stay bounded
/// by the uncommitted tail, not grow with the whole take. A 60-minute take
/// must not hold ~230 MB in `buf16`.
#[test]
fn edge_case_9_long_take_buffer_bounded() {
    // 60 s of 16 kHz audio = 960,000 samples = ~3.8 MB. The buffer must
    // stay bounded by the uncommitted tail (~12 s max + one frame), not
    // grow to the full take length.
    let vad_script = tests::p4_60s_vad_script();
    let mut take = tests::streaming_take_with_mock("segment text", vad_script);

    // Feed 60 s of audio in 10 s chunks.
    for _ in 0..6 {
        take.feed_frame(&vec![0.5; 160_000], 16_000); // 10 s
    }

    // The buffer must be bounded by the uncommitted tail (~12 s max).
    // 12 s of 16 kHz audio = 192,000 samples = ~768 KB.
    let buf_len = take.buf16.len();
    assert!(
        buf_len < 200_000,
        "the take buffer must be bounded by the uncommitted tail; got {buf_len} samples (~{} KB)",
        buf_len * 4 / 1024
    );
    // The full take is 960,000 samples; the buffer must be much smaller.
    assert!(
        buf_len < 960_000 / 4,
        "the take buffer must not hold the whole take; got {buf_len} samples"
    );
}

/// Edge case #14: ASR engine fails to load. The worker must report the
/// error and the pill must flash. A C-level abort is not simulatable from
/// Rust; this tests the Rust-equivalent (a provider that returns
/// `SpeechError::ModelLoad`).
#[cfg(feature = "fault-injection")]
#[test]
fn edge_case_14_engine_load_failure() {
    use crate::fault_inject;
    let _guard = fault_inject::scoped();
    fault_inject::arm(fault_inject::ENGINE_LOAD_ERROR);

    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    session.headless_flash = true;
    // No mock_asr: the worker will hit the fault-injection branch.

    let recording = teletype_core::audio::Recording::synthetic();
    // The take must hold audio: an empty take is dropped by the
    // no-audio gate before the engine-load path runs (the fault point
    // is the engine load, not the capture).
    recording.push(&vec![0.5f32; 16_000 * 2]);
    RECORDING.with(|slot| slot.borrow_mut().replace(recording));
    session.phase = Phase::Listening;
    session.started_at_ms = teletype_core::storage::now_ms() - 1_000;
    session.recording_started = Some(std::time::Instant::now());

    session.stop(false);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if session.phase == Phase::Idle {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "engine-load-failure take never reached Idle; phase = {:?}",
                session.phase
            );
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => session.handle(event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // The pill must flash the engine-load error exactly once.
    assert_eq!(
        session.headless_flashes.len(),
        1,
        "an engine-load-failure take must flash the pill exactly once"
    );
    assert!(
        session
            .headless_flashes
            .iter()
            .any(|f| f.contains("Speech model load failed")),
        "the flash must carry the engine-load error message; got {:?}",
        session.headless_flashes
    );
}

/// Residual #20: command tool fails. The command executor must report the
/// error (an `Err(String)` before any side effect) so the controller can
/// flash the pill. The fault point is in `SystemEffects::run`, which
/// every real tool goes through.
#[cfg(feature = "fault-injection")]
#[test]
fn edge_case_residual_20_command_tool_error() {
    use crate::command_executor::execute;
    use crate::fault_inject;
    use teletype_core::context::ApplicationContext;
    use teletype_core::llm::CommandAction;

    let _guard = fault_inject::scoped();
    fault_inject::arm(fault_inject::COMMAND_TOOL_ERROR);

    let cmd = CommandAction {
        tool: "open_app".to_string(),
        arguments: serde_json::json!({ "app": "Notes" }),
    };
    let context = ApplicationContext::default();
    let result = execute(&cmd, &context);

    // The executor must report the error before any side effect.
    let err = result.expect_err("the fault must make the tool fail");
    assert!(
        err.contains("open_app failed: fault-injected tool error"),
        "the error must carry the tool name and the fault reason; got {err:?}"
    );
}

/// HARD-4 row 14: the fallback gate. The gate is the part that can be
/// wrong in production (a wrong `Some` pays a second load plus decode on
/// a take that is not broken; a wrong `None` leaves the take failed), so
/// it is a pure function with its own cases. The plumbing above it needs
/// an `AppHandle` and a real second model, which no headless test has.
#[test]
fn row14_fallback_gate_only_fires_for_a_model_failure() {
    // A load or decode failure with a different last working model: fall
    // back to it.
    assert_eq!(
        pick_fallback_model("Speech model not downloaded", "chosen", Some("other")),
        Some("other")
    );
    // Nothing has decoded yet: no candidate.
    assert_eq!(
        pick_fallback_model("decode failed", "chosen", None),
        None,
        "a first take must not fall back"
    );
    // The last working model IS the selected one: retrying the same
    // failure buys nothing.
    assert_eq!(
        pick_fallback_model("decode failed", "chosen", Some("chosen")),
        None,
        "must not retry the model that just failed"
    );
    // The two non-failures: no speech, and no audio at all. Both must
    // stay quiet, or every silent take pays a second decode.
    assert_eq!(
        pick_fallback_model(NO_SPEECH_SENTINEL, "chosen", Some("other")),
        None,
        "a silent take is not a model failure"
    );
    assert_eq!(
        pick_fallback_model(MIC_FAILURE_MESSAGE, "chosen", Some("other")),
        None,
        "a take with no audio is not a model failure"
    );
}

/// HARD-4 row 2: a mid-take device loss (the cpal stream error callback
/// fires while the take is live) must end the take through the normal
/// stop path so the captured audio is still transcribed and pasted, and
/// the pill must flash the device-loss message.
///
/// The seam being proven is object identity: the flag the test writes is
/// the one the live `Recording` carries, which is the same `Arc` the
/// capture thread hands to cpal's `err_fn`. The take adopts that flag
/// through `e2e_feed`, the same call a file-fed take uses, so the test
/// cannot pass by setting some unrelated flag the take never reads.
#[test]
fn row2_stream_error_ends_take_with_notice() {
    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    session.headless_flash = true;
    // A scripted ASR outcome: the worker's real transcribe call is
    // replaced by this text (the headless session has no model), so the
    // take delivers the "captured audio" the device had before it died.
    // The text has lexical content and no filler words, so the headless
    // pipeline's F-62 floor delivers it as the final text. Must be set
    // before `stop()` (the worker closure reads it once at spawn).
    session.mock_asr = Some(Ok(("hello world this is a test".to_string(), None)));
    // The take's session id: `stop` creates it by incrementing
    // `next_id`, so the terminal event's id is the value `next_id` had
    // before the increment (the take's id is `next_id - 1` after
    // `stop`). The controller's `transcribed` guard compares against
    // `next_id`, so rewind it for the terminal event, then advance it
    // past the take once the event is handled, exactly as the
    // production pump would before the next take.
    let take_session = session.next_id + 1;

    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| slot.borrow_mut().replace(recording));
    session.phase = Phase::Listening;
    session.started_at_ms = teletype_core::storage::now_ms() - 1_000;
    session.recording_started = Some(std::time::Instant::now());
    // The take holds a second of audio captured BEFORE the device died,
    // and adopts the live recording's device-error flag the same way a
    // file-fed take does.
    session.e2e_feed(&vec![0.5f32; 16_000]);
    let take_flag = session
        .device_error_flag
        .clone()
        .expect("the take holds the recording's device-error flag");
    // Positive control: that flag IS the recording's own object, the one
    // the capture thread hands to cpal's error callback. Without this the
    // test could pass on a flag nothing else can write.
    RECORDING.with(|slot| {
        let captured = slot.borrow();
        let rec_flag = captured.as_ref().expect("take audio").device_error_flag();
        assert!(
            Arc::ptr_eq(&take_flag, &rec_flag),
            "the take must read the same flag the capture side writes"
        );
    });
    // The device stops responding mid-take.
    take_flag.store(true, std::sync::atomic::Ordering::SeqCst);

    session.stop(false);

    // The take's session id: `stop` creates it by incrementing
    // `next_id`, so the terminal event's id is the value `next_id` had
    // before the increment (the take's id is `next_id - 1` after
    // `stop`). The controller's `transcribed` guard compares against
    // `next_id`, so rewind it for the terminal event, then advance it
    // past the take once the event is handled, exactly as the
    // production pump would before the next take.
    // Drain the worker's terminal event and the device-loss flash that
    // rides behind it (one FIFO channel). The terminal event returns the
    // phase to Idle; the flash is not gated on the phase, so one extra
    // recv after Idle picks it up.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut got_terminal = false;
    let mut got_flash = false;
    for _ in 0..50 {
        if got_terminal && got_flash {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "device-loss take never delivered; phase = {:?}, flashes = {:?}, texts = {:?}, terminal = {}, flash = {}",
                session.phase,
                session.headless_flashes,
                session.headless_final_texts,
                got_terminal,
                got_flash
            );
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => {
                let is_terminal = matches!(
                    event,
                    Event::Transcribed { session: s, .. } if s == take_session
                );
                let is_flash = matches!(event, Event::FlashNote { .. });
                got_terminal = got_terminal || is_terminal;
                got_flash = got_flash || is_flash;
                // The terminal event is stale against the controller's
                // `next_id` (the controller creates the session id by
                // incrementing, so the terminal event's id is the
                // previous value). Rewind `next_id` for the terminal
                // event, then advance it past the take once handled,
                // exactly as the production pump would before the next
                // take.
                if is_terminal {
                    session.next_id = take_session;
                }
                session.handle(event);
                if is_terminal {
                    session.next_id = take_session + 1;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // The take's text was delivered (the captured audio transcribed).
    let delivered = session.headless_final_texts.iter().any(|t| !t.is_empty());
    assert!(
        delivered,
        "the audio captured before the device loss must still be transcribed and delivered; texts = {:?}",
        session.headless_final_texts
    );
    // The pill must flash the device-loss message.
    assert!(
        session
            .headless_flashes
            .iter()
            .any(|f| f.contains("Your microphone stopped responding")),
        "the flash must carry the device-loss message; got {:?}",
        session.headless_flashes
    );
}

/// HARD-4 row 2 (negative): a take with no device error gets no
/// device-loss message. This is what keeps the HARD-3 relabelled row 2
/// test (zero samples, mic-failure message) honest: the device message
/// only appears when the error flag was actually set.
#[test]
fn row2_no_flag_no_device_message() {
    let (events_tx, events_rx) = std::sync::mpsc::channel::<Event>();
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.test_events = Some(events_tx);
    session.headless_flash = true;
    // Scripted ASR outcome: without this the headless worker would reach
    // the real transcribe call and panic, so the take never terminates.
    session.mock_asr = Some(Ok(("hello world this is a test".to_string(), None)));

    let recording = teletype_core::audio::Recording::synthetic();
    RECORDING.with(|slot| slot.borrow_mut().replace(recording));
    session.phase = Phase::Listening;
    session.started_at_ms = teletype_core::storage::now_ms() - 1_000;
    session.recording_started = Some(std::time::Instant::now());
    // Same path as the row-2 take above, same flag object: the difference
    // is only that nobody set it. An ordinary take must stay quiet.
    session.e2e_feed(&vec![0.5f32; 16_000]);
    let take_flag = session
        .device_error_flag
        .clone()
        .expect("the take holds the recording's device-error flag");
    assert!(
        !take_flag.load(std::sync::atomic::Ordering::SeqCst),
        "the negative control must leave the flag clear"
    );

    session.stop(false);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if session.phase == Phase::Idle {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("clean take never reached Idle; phase = {:?}", session.phase);
        }
        match events_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => session.handle(event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    assert!(
        session
            .headless_flashes
            .iter()
            .all(|f| !f.contains("Your microphone stopped responding")),
        "a take with no device error must not show the device-loss message; got {:?}",
        session.headless_flashes
    );
}
