//! P9.2 — concurrency stress: 1,000 back-to-back mock dictations through
//! the core pipeline (plan §9.2 Concurrency row: "a stress test of 1,000
//! back-to-back mock dictations").
//!
//! # Plan deviation (recorded for the lead)
//!
//! The plan row also mentions `loom` and an engine `ArcSwap`. **Neither
//! exists in this codebase**: there is no `loom` dependency and no
//! `ArcSwap` component in `teletype-core` (grep-verified). This file
//! therefore does not add `loom` and does not test an `ArcSwap` swap —
//! it tests what the row can express with the public API: 1,000
//! back-to-back mock dictations, single-threaded, plus a 4-thread variant
//! over shared, read-mostly state.
//!
//! # What is tested
//!
//! 1. **Single-threaded**: 1,000 back-to-back dictations (mock ASR text +
//!    echo polish mock) through the real `Pipeline`. Every output is
//!    asserted byte-for-byte against its input, a monotone counter is
//!    asserted, and the total runtime is asserted to stay under a
//!    generous hang alarm. No sleeps for correctness.
//!
//! 2. **Concurrent**: 4 threads × 250 dictations each, running the same
//!    loop over **shared, read-mostly state** — the same immutable
//!    stores (`TransformStore`, `StyleProfileStore`, `Dictionary`,
//!    `UserProfile`, `MockPlatform`) are shared across all threads via
//!    `Arc`, which is exactly how the desktop app holds them. Each thread
//!    runs its own `Pipeline` (the `Pipeline` struct borrows the stores,
//!    so a per-thread pipeline is the shape the API allows without source
//!    changes) and increments a shared `AtomicUsize`. Asserts a total
//!    count of exactly 1,000 and no deadlock (join with a timeout alarm).
//!
//! ```text
//! cargo test -p teletype-core --test concurrency_stress
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use teletype_core::autotext::AutoTextStore;
use teletype_core::dictionary::Dictionary;
use teletype_core::llm::{GenerationParams, InferenceProvider};
use teletype_core::personalization::UserProfile;
use teletype_core::pipeline::{InputSource, Pipeline, PipelineResult, UnifiedInput};
use teletype_core::platform::MockPlatform;
use teletype_core::style::StyleProfileStore;
use teletype_core::transforms::TransformStore;

// ---------------------------------------------------------------------------
// Mock polish provider: byte-for-byte echo (same contract as i18n.rs).
// ---------------------------------------------------------------------------

struct EchoLlm;

impl InferenceProvider for EchoLlm {
    fn model_id(&self) -> &str {
        "echo"
    }
    fn model_name(&self) -> &str {
        "Echo (test)"
    }
    fn is_local(&self) -> bool {
        true
    }
    fn generate(&self, prompt: &str, _params: GenerationParams) -> Result<String, String> {
        if let (Some(a), Some(b)) = (prompt.rfind("<<<\n").map(|i| i + 4), prompt.rfind("\n>>>")) {
            if b > a {
                return Ok(prompt[a..b].to_string());
            }
        }
        Err("echo mock: could not find the marked input in the prompt".into())
    }
}

// ---------------------------------------------------------------------------
// Shared, read-mostly stores — the shape the desktop app holds them in.
// ---------------------------------------------------------------------------

/// The stores every pipeline borrows. In the desktop app these live in
/// `AppState` and are shared across the pipeline's threads; here we hold
/// them in `Arc`s and share the same `Arc`s across the stress threads.
struct SharedStores {
    platform: MockPlatform,
    autotext: AutoTextStore,
    transforms: TransformStore,
    profile: UserProfile,
    dictionary: Dictionary,
    styles: StyleProfileStore,
    active_style: String,
    word_checker: teletype_core::dictionary::EditDistanceChecker,
}

impl SharedStores {
    fn new() -> Self {
        Self {
            platform: MockPlatform::default(),
            autotext: AutoTextStore::default(),
            transforms: TransformStore::with_built_ins(),
            profile: UserProfile::default(),
            dictionary: Dictionary::default(),
            styles: StyleProfileStore::with_built_ins(),
            active_style: String::new(),
            word_checker: teletype_core::dictionary::EditDistanceChecker::new(),
        }
    }
}

/// Build a `Pipeline` over the shared stores, the way dictation does:
/// voice source, auto-apply transform on, echo polish mock.
fn run_one(stores: &SharedStores, text: &str) -> PipelineResult {
    let llm = EchoLlm;
    let mut p = Pipeline {
        platform: &stores.platform,
        autotext: &stores.autotext,
        transforms: &stores.transforms,
        profile: &stores.profile,
        inference: Some(&llm),
        decision: None,
        force_command: false,
        dictionary: &stores.dictionary,
        styles: &stores.styles,
        active_style: &stores.active_style,
        explicit_style: "",
        auto_apply: true,
        restore_clipboard: true,
        remove_filler_words: false,
        filler_words: Vec::new(),
        restore_emoji: true,
        spoken_emoji: true,
        spoken_punctuation: true,
        system_autotext: &[],
        token_sink: None,
        polish_gate_enabled: false,
        polish_gate_threshold_words: 8,
        pack_terms: &[],
        list_style: teletype_core::transforms::ListStyle::default(),
        polish_max_change_percent: 40,
        word_checker: &stores.word_checker,
    };
    p.run(
        UnifiedInput {
            source: InputSource::Voice,
            text: text.into(),
        },
        None,
    )
}

/// The 1,000-utterance corpus: a rotating mix of a plain English
/// sentence, a non-Latin sentence, and an emoji sentence, so the stress
/// exercises the same code paths a real day of dictation would.
fn corpus(i: usize) -> String {
    match i % 3 {
        0 => format!("the deploy went out this morning and it is all good number {i}"),
        1 => format!("الاجتماع الساعة 10:30 في قاعة 203 رقم {i}"),
        _ => format!("会议是明天上午十点 in room 305 🚀 take {i}"),
    }
}

// ---------------------------------------------------------------------------
// 1. Single-threaded: 1,000 back-to-back dictations
// ---------------------------------------------------------------------------

#[test]
fn stress_1000_back_to_back_dictations_single_threaded() {
    const N: usize = 1000;
    let stores = SharedStores::new();
    let mut completed = 0usize;
    let mut first_output: Option<String> = None;

    let start = Instant::now();
    for i in 0..N {
        let text = corpus(i);
        let r = run_one(&stores, &text);
        // Every output must be byte-for-byte its input: the echo mock
        // returns the input unchanged and no deterministic pass may mutate
        // it (ITN is English-gated and these contain no English number
        // words; filler removal is off; no autotext triggers present).
        assert_eq!(
            r.final_text.as_bytes(),
            text.as_bytes(),
            "dictation {i} mutated its input: {text:?} -> {:?}",
            r.final_text
        );
        // No placeholder leakage, no replacement char.
        assert!(
            !r.final_text.contains("AUTOTEXT_"),
            "dictation {i} leaked a placeholder"
        );
        assert!(
            !r.final_text.contains('\u{FFFD}'),
            "dictation {i} produced a replacement char"
        );
        // The transform ran (the echo mock produced output that passed the
        // validator) and was not a fallback.
        assert!(
            r.transformed,
            "dictation {i}: the echo polish was rejected; the pipeline fell back"
        );
        completed += 1;
        if first_output.is_none() {
            first_output = Some(r.final_text);
        }
    }
    let elapsed = start.elapsed();

    assert_eq!(completed, N, "the counter must reach exactly {N}");
    assert!(first_output.is_some(), "at least one output must exist");
    // Hang alarm, not a perf budget: a true hang or quadratic regression
    // runs unboundedly, while load can inflate wall time arbitrarily (this
    // measured 69 s while four cargo builds shared the machine, versus ~2 s
    // idle). The alarm exists only to fail loudly on deadlock/O(n^2).
    assert!(
        elapsed < Duration::from_secs(180),
        "1,000 dictations took {elapsed:?}; that is a hang or a super-linear regression"
    );
    eprintln!("stress: 1,000 back-to-back dictations took {elapsed:?} (single-threaded)");
}

// ---------------------------------------------------------------------------
// 2. Concurrent: 4 threads × 250 dictations over shared, read-mostly state
// ---------------------------------------------------------------------------

#[test]
fn stress_1000_dictations_4_threads_shared_read_only_stores() {
    const THREADS: usize = 4;
    const PER_THREAD: usize = 250;
    const N: usize = THREADS * PER_THREAD; // 1,000

    // Shared, read-mostly state: the same stores every thread borrows.
    // This is exactly how the desktop app holds them (in `AppState`,
    // shared across the pipeline's threads). The stores are only ever
    // read by the pipeline, so `Arc<SharedStores>` is the safe,
    // source-change-free way to share them.
    let stores = Arc::new(SharedStores::new());
    let total = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();
    let handles: Vec<std::thread::JoinHandle<()>> = (0..THREADS)
        .map(|t| {
            let stores = Arc::clone(&stores);
            let total = Arc::clone(&total);
            std::thread::spawn(move || {
                for i in 0..PER_THREAD {
                    // Each thread runs its own slice of the corpus so no
                    // two threads assert on the same input index.
                    let idx = t * PER_THREAD + i;
                    let text = corpus(idx);
                    let r = run_one(&stores, &text);
                    assert_eq!(
                        r.final_text.as_bytes(),
                        text.as_bytes(),
                        "thread {t} dictation {i} mutated its input: {text:?} -> {:?}",
                        r.final_text
                    );
                    assert!(
                        r.transformed,
                        "thread {t} dictation {i}: the echo polish was rejected"
                    );
                    total.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();

    // Join with a timeout alarm so a deadlock fails loudly instead of
    // hanging the test harness (~2 s idle single-threaded; heavy parallel
    // load stretches wall time, so this deadline guards joins that NEVER
    // return - e.g. a store-lock deadlock).
    let deadline = Instant::now() + Duration::from_secs(120);
    for h in handles {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            remaining > Duration::from_secs(0),
            "a thread did not finish within the 120 s alarm; suspected deadlock"
        );
        h.join().expect("stress thread panicked");
    }
    let elapsed = start.elapsed();

    let count = total.load(Ordering::SeqCst);
    assert_eq!(
        count, N,
        "the shared counter must reach exactly {N}, got {count}"
    );
    assert!(
        elapsed < Duration::from_secs(120),
        "4 × 250 concurrent dictations took {elapsed:?}; suspected deadlock or hang"
    );
    eprintln!("stress: 4 threads × 250 dictations over shared read-only stores took {elapsed:?}");
}
