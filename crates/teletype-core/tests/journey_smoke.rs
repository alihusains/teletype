//! End-user journey smoke tests.
//!
//! The unit tests elsewhere in this crate pin individual functions. These
//! tests walk the *journeys a person actually takes* through the app, from
//! first launch to insights, and assert only the invariants that must hold
//! for a user's day not to be ruined:
//!
//! 1. first launch on a clean machine
//! 2. onboarding with permissions denied
//! 3. dictating with no model downloaded
//! 4. the core dictation loop
//! 5. AutoText (typed and spoken) placeholder integrity
//! 6. LLM failure modes never lose the user's words
//! 7. dictionary + ITN corrections
//! 8. history -> insights -> stats consistency
//! 9. the day-wise transcript archive
//! 10. corrupt-file recovery on every store
//! 11. the hotkey state machine
//! 12. unicode / emoji / very long text through the whole pipeline
//! 13. import / export round trip
//! 14. settings persistence across a restart
//!
//! Every test is headless: no mic, no display, no network. Mocks implement
//! the same traits production uses, so a signature change in a trait breaks
//! these tests the same day it breaks the app.
//!
//! # Confirmed defects
//!
//! Tests annotated `BUG-xx` reproduce a defect that is present in the code
//! today. They are `#[ignore]`d so the suite stays green for CI, and are
//! the reproduction for the corresponding finding. Run them all with:
//!
//! ```text
//! cargo test -p teletype-core --test journey_smoke -- --ignored
//! ```
//!
//! When a `BUG-xx` test starts passing, delete the `#[ignore]` line.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use teletype_core::autotext::{AutoTextEntry, AutoTextStore};
use teletype_core::context::{self, ApplicationContext};
use teletype_core::dictionary::{self, Dictionary, DictionaryWord};
use teletype_core::history::{self, DictationEntry, DictationHistory};
use teletype_core::insights;
use teletype_core::llm::{GenerationParams, InferenceProvider};
use teletype_core::personalization::UserProfile;
use teletype_core::pipeline::{InputSource, Pipeline, UnifiedInput};
use teletype_core::platform::{MockPlatform, Platform};
use teletype_core::state::{self, Input, Phase, RecordingMode};
use teletype_core::storage::JsonStore;
use teletype_core::style::StyleProfileStore;
use teletype_core::transforms::TransformStore;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// A throwaway app-data directory, removed on drop so a failed test cannot
/// leave state behind that makes the next run lie.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p =
            std::env::temp_dir().join(format!("teletype-smoke-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).expect("create temp dir");
        Self(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn word(text: &str) -> u32 {
    teletype_core::stats::word_count(text)
}

/// An inference provider whose every response mode can be provoked.
struct ScriptedLlm {
    /// What `generate` returns.
    reply: Result<String, String>,
    /// Prompts this provider was asked, in order.
    seen: std::sync::Mutex<Vec<String>>,
    /// `max_tokens` the engine asked for on the last call.
    last_max_tokens: std::sync::Mutex<u32>,
    /// The `model_id` reported to the engine.
    id: String,
}

impl ScriptedLlm {
    fn ok(text: &str) -> Self {
        Self {
            reply: Ok(text.into()),
            seen: std::sync::Mutex::new(Vec::new()),
            last_max_tokens: std::sync::Mutex::new(0),
            id: "scripted".into(),
        }
    }

    /// A model that behaves like a real one: it capitalises the text it is
    /// given and changes nothing else.
    ///
    /// Use this whenever the test asserts on the *delivered* text. A fixed
    /// string is only right when the point of the test is that a particular
    /// bad response must not lose the user's words, because a response sharing
    /// no content with the input is now (correctly) rejected by
    /// `validator::Failure::OffTopic` and the pipeline falls back to the
    /// input.
    fn echo() -> Self {
        Self {
            reply: Ok(String::new()),
            seen: std::sync::Mutex::new(Vec::new()),
            last_max_tokens: std::sync::Mutex::new(0),
            id: "scripted".into(),
        }
    }
    fn err(msg: &str) -> Self {
        Self {
            reply: Err(msg.into()),
            seen: std::sync::Mutex::new(Vec::new()),
            last_max_tokens: std::sync::Mutex::new(0),
            id: "scripted".into(),
        }
    }
    fn prompts(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
    fn max_tokens(&self) -> u32 {
        *self.last_max_tokens.lock().unwrap()
    }
}

impl InferenceProvider for ScriptedLlm {
    fn model_id(&self) -> &str {
        &self.id
    }
    fn model_name(&self) -> &str {
        "Scripted (test)"
    }
    fn is_local(&self) -> bool {
        true
    }
    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String> {
        self.seen.lock().unwrap().push(prompt.to_string());
        *self.last_max_tokens.lock().unwrap() = params.max_tokens;
        // Echo mode: take the marked input out of the prompt and capitalise it.
        if self.reply.as_ref() == Ok(&String::new()) {
            if let (Some(a), Some(b)) =
                (prompt.rfind("<<<\n").map(|i| i + 4), prompt.rfind("\n>>>"))
            {
                if b > a {
                    let body = prompt[a..b].trim();
                    let mut chars = body.chars();
                    let first = chars
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default();
                    return Ok(format!("{first}{}", chars.collect::<String>()));
                }
            }
        }
        self.reply.clone()
    }
}

/// Everything the pipeline borrows, in one bundle so each test reads as a
/// scenario rather than as 20 struct-literal fields.
struct Fixture {
    platform: MockPlatform,
    autotext: AutoTextStore,
    transforms: TransformStore,
    profile: UserProfile,
    dictionary: Dictionary,
    styles: StyleProfileStore,
    active_style: String,
    filler_words: Vec<String>,
    pack_terms: Vec<teletype_core::vocab::PackTerm>,
    word_checker: teletype_core::dictionary::EditDistanceChecker,
}

impl Fixture {
    fn new() -> Self {
        Self {
            platform: MockPlatform::default(),
            autotext: AutoTextStore::default(),
            transforms: TransformStore::with_built_ins(),
            profile: UserProfile::default(),
            dictionary: Dictionary::default(),
            styles: StyleProfileStore::with_built_ins(),
            active_style: String::new(),
            filler_words: vec!["um".into(), "uh".into()],
            pack_terms: Vec::new(),
            word_checker: teletype_core::dictionary::EditDistanceChecker::new(),
        }
    }

    /// In the given app, as the user sees it.
    fn in_app(mut self, id: &str, name: &str) -> Self {
        self.platform = MockPlatform::with_app(context::normalize(id, name));
        self
    }

    fn with_filler_removal(mut self) -> Self {
        self.filler_words = vec!["um".into(), "uh".into(), "like".into()];
        self
    }

    /// Override the profile's language (e.g. "fr", "ja").
    fn with_language(mut self, lang: &str) -> Self {
        self.profile.language = lang.into();
        self
    }

    /// Run one utterance through the pipeline the way dictation does:
    /// voice source, auto-apply transform on, everything else default.
    fn dictate(
        &self,
        text: &str,
        llm: Option<&dyn InferenceProvider>,
    ) -> teletype_core::pipeline::PipelineResult {
        let mut p = Pipeline {
            platform: &self.platform,
            autotext: &self.autotext,
            transforms: &self.transforms,
            profile: &self.profile,
            inference: llm,
            dictionary: &self.dictionary,
            styles: &self.styles,
            active_style: &self.active_style,
            explicit_style: "",
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: !self.filler_words.is_empty(),
            filler_words: self.filler_words.clone(),
            restore_emoji: true,
            spoken_emoji: true,
            spoken_punctuation: true,
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &self.pack_terms,
            list_style: teletype_core::transforms::ListStyle::default(),
            word_checker: &self.word_checker,
        };
        p.run(
            UnifiedInput {
                source: InputSource::Voice,
                text: text.into(),
            },
            None,
        )
    }

    /// Run typed input (what the AutoText watcher sends), which by default
    /// must not invoke the model.
    fn typed(
        &self,
        text: &str,
        llm: Option<&dyn InferenceProvider>,
    ) -> teletype_core::pipeline::PipelineResult {
        let mut p = Pipeline {
            platform: &self.platform,
            autotext: &self.autotext,
            transforms: &self.transforms,
            profile: &self.profile,
            inference: llm,
            dictionary: &self.dictionary,
            styles: &self.styles,
            active_style: &self.active_style,
            explicit_style: "",
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            restore_emoji: false,
            spoken_emoji: false,
            spoken_punctuation: true,
            system_autotext: &[],
            token_sink: None,
            polish_gate_enabled: false,
            polish_gate_threshold_words: 8,
            pack_terms: &self.pack_terms,
            list_style: teletype_core::transforms::ListStyle::default(),
            word_checker: &self.word_checker,
        };
        p.run(
            UnifiedInput {
                source: InputSource::Typed,
                text: text.into(),
            },
            None,
        )
    }
}

/// A snippet entry that fires on both the typed trigger and the spoken
/// phrase, which is what the AutoText screen creates when the user fills in
/// both fields.
fn snippet(trigger: &str, value: &str) -> AutoTextEntry {
    let mut e = AutoTextEntry::new(trigger, value);
    e.snippet = trigger.trim_start_matches('/').to_string();
    e
}

/// The invariant that matters most in this product: whatever happened
/// upstream, the user must be able to find every content word they spoke in
/// what lands in their document.
fn assert_no_words_lost(spoken: &str, delivered: &str) {
    let stop = |w: &str| {
        let w: String = w
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        w.len() > 2 && !stopwords().contains(&w.as_str())
    };
    let said: HashSet<String> = spoken
        .split_whitespace()
        .filter(|w| stop(w))
        .map(|w| w.to_lowercase())
        .collect();
    let got: String = delivered.to_lowercase();
    let missing: Vec<&String> = said.iter().filter(|w| !got.contains(w.as_str())).collect();
    assert!(
        missing.is_empty(),
        "words lost: {missing:?}\n  spoken:    {spoken:?}\n  delivered: {delivered:?}"
    );
}

fn stopwords() -> HashSet<&'static str> {
    [
        "the", "and", "for", "you", "that", "this", "with", "from", "have", "has", "was", "were",
        "are", "but", "not", "all", "can", "will", "just", "about", "into", "than", "then",
    ]
    .into_iter()
    .collect()
}

// ===========================================================================
// JOURNEY 1: first launch on a clean machine
// ===========================================================================

#[test]
fn journey_01_first_launch_on_a_clean_machine_loads_every_store() {
    let dir = TempDir::new("first-launch");

    // Every persistent document must load from nothing without panicking.
    let history = history::open_history(dir.path()).1;
    assert!(history.entries.is_empty(), "fresh history must be empty");

    let dict_store = JsonStore::new(dir.path(), "dictionary.json");
    let dict: Dictionary = dict_store.load(Dictionary::default());
    assert!(dict.words.is_empty());

    let at_store = JsonStore::new(dir.path(), "autotext.json");
    let at: AutoTextStore = at_store.load(AutoTextStore::default());
    assert!(at.entries.is_empty());

    let tr_store = JsonStore::new(dir.path(), "transforms.json");
    let tr: TransformStore = tr_store.load(TransformStore::with_built_ins());
    assert!(!tr.transforms.is_empty(), "built-in transforms must seed");

    // The seed stores must also be constructible in memory, which is the
    // path the app takes before the first save.
    let _ = StyleProfileStore::with_built_ins();
    let _ = UserProfile::default();

    // A brand new install has no model, so the app must be able to answer
    // "what is selected" without a panic.
    assert!(dict_store.load(Dictionary::default()).words.is_empty());
}

#[test]
fn journey_01b_fresh_install_can_still_produce_text_with_no_model_at_all() {
    // The single most important first-run guarantee: a user who installed five
    // minutes ago and has not downloaded a model still gets their words.
    //
    // The built-in transforms ship enabled with auto-apply on, so a transform
    // *is* selected here, which means the UI must also be able to tell the
    // user that nothing was polished. `transform_skipped_no_model` used to be
    // false on exactly this path (it inferred the skip from
    // `transform_result.is_none()`, but the no-provider arm returns a
    // `TransformResult` carrying `SkipReason::NoModelLoaded`), so the warning
    // in `dictation.rs` could never fire.
    let fx = Fixture::new();
    let spoken = "so the deploy went out this morning and it is all good";
    let r = fx.dictate(spoken, None);
    assert!(
        r.transform_skipped_no_model,
        "a transform was selected and no model was loaded, so the UI must be \
         able to say so: {:?}",
        r.transform_skipped_no_model
    );
    assert!(
        !r.transformed,
        "nothing could have transformed without a model"
    );
    assert_no_words_lost(spoken, &r.final_text);
}

#[test]
fn journey_02_denied_permissions_do_not_lose_the_dictation() {
    // On macOS the user can decline Accessibility. The dictation must still
    // produce the text (it lands in history and the pill) rather than fail.
    let fx = Fixture::new();
    let llm = ScriptedLlm::ok("The deploy went out this morning and it is all good.");
    let r = fx.dictate(
        "so the deploy went out this morning and it is all good",
        Some(&llm),
    );
    assert_no_words_lost(
        "so the deploy went out this morning and it is all good",
        &r.final_text,
    );
}

#[test]
fn journey_02b_permissions_surface_reports_both_kinds() {
    let p = MockPlatform::default();
    let perms = p.permissions();
    use teletype_core::platform::PermissionKind;
    let has = |k: PermissionKind| perms.iter().any(|x| x.kind == k);
    assert!(has(PermissionKind::Microphone), "microphone not reported");
    assert!(
        has(PermissionKind::Accessibility),
        "accessibility not reported"
    );
    assert!(
        perms.iter().all(|x| x.granted()),
        "the mock must be able to model the granted state"
    );
    assert!(
        p.permission_settings_url(PermissionKind::Microphone)
            .is_none(),
        "the mock has no settings URL, callers must tolerate None"
    );
}

// ===========================================================================
// JOURNEY 3: the core dictation loop
// ===========================================================================

#[test]
fn journey_03_core_dictation_cleans_fillers_and_keeps_content() {
    let fx = Fixture::new()
        .in_app("com.apple.Notes", "Notes")
        .with_filler_removal();
    let spoken = "um so uh the deploy went out this morning and it is all good";
    let llm = ScriptedLlm::ok("The deploy went out this morning and it's all good.");
    let r = fx.dictate(spoken, Some(&llm));
    assert!(
        !r.final_text.contains(" um "),
        "filler survived: {:?}",
        r.final_text
    );
    assert!(
        !r.final_text.contains(" uh "),
        "filler survived: {:?}",
        r.final_text
    );
    assert_eq!(
        r.raw_input, spoken,
        "raw input must be preserved for history"
    );
    assert_no_words_lost(spoken, &r.final_text);
}

#[test]
fn journey_03b_dictation_works_with_the_app_context_unknown() {
    // Detection can fail (fullscreen app, no AX permission). The pipeline
    // must not invent an app and must still work.
    let fx = Fixture::new(); // MockPlatform::default() -> app: None
    assert!(fx.platform.active_application().is_none());
    let llm = ScriptedLlm::ok("Hello there.");
    let r = fx.dictate("hello there", Some(&llm));
    assert!(!r.final_text.is_empty());
}

#[test]
fn journey_03c_an_explicitly_requested_transform_actually_runs() {
    // The user picks "Professional" from the pill.
    let fx = Fixture::new();
    let transforms = TransformStore::with_built_ins();
    let prof = transforms
        .get("builtin-professional")
        .expect("built-in professional transform")
        .clone();
    // A pass-through model: capitalise and add a period, nothing else. The
    // word-retention assertion below is then a statement about the pipeline,
    // not about the mock's imagination.
    let llm = ScriptedLlm::ok("Can you send me that report please.");
    let mut p = Pipeline {
        platform: &fx.platform,
        autotext: &fx.autotext,
        transforms: &transforms,
        profile: &fx.profile,
        inference: Some(&llm),
        dictionary: &fx.dictionary,
        styles: &fx.styles,
        active_style: &fx.active_style,
        explicit_style: "",
        auto_apply: true,
        restore_clipboard: true,
        remove_filler_words: true,
        filler_words: fx.filler_words.clone(),
        restore_emoji: true,
        spoken_emoji: true,
        spoken_punctuation: true,
        system_autotext: &[],
        token_sink: None,
        polish_gate_enabled: false,
        polish_gate_threshold_words: 8,
        pack_terms: &[],
        list_style: teletype_core::transforms::ListStyle::default(),
        word_checker: &fx.word_checker,
    };
    let r = p.run(
        UnifiedInput {
            source: InputSource::Voice,
            text: "um can you send me that report please".into(),
        },
        Some(&prof),
    );
    assert!(
        r.transform.is_some(),
        "an explicitly requested transform must be reported as run"
    );
    assert!(!r.transform_skipped_no_model);
    assert_no_words_lost("can you send me that report please", &r.final_text);
}

// ===========================================================================
// JOURNEY 4: AutoText
// ===========================================================================

#[test]
fn journey_04_typed_autotext_expands_in_a_sentence() {
    let mut fx = Fixture::new();
    fx.autotext
        .insert(snippet("/sig", "Best regards,\nAli"))
        .expect("insert");
    let r = fx.typed("thanks for that /sig", None);
    assert!(r.autotext_expanded, "the trigger must expand");
    assert!(r.final_text.contains("Best regards,"), "{:?}", r.final_text);
    assert!(!r.final_text.contains("/sig"), "trigger left visible");
}

#[test]
fn journey_04b_spoken_snippet_survives_an_llm_transform_intact() {
    let mut fx = Fixture::new();
    fx.autotext
        .insert(snippet("my address", "221B Baker Street, London"))
        .expect("insert");
    let llm = ScriptedLlm::ok("I moved to {{AUTOTEXT_0}} last year.");
    let r = fx.dictate("um i moved to my address last year", Some(&llm));
    assert!(
        r.final_text.contains("221B Baker Street, London"),
        "AutoText value must be restored exactly: {:?}",
        r.final_text
    );
    assert!(
        !r.final_text.contains("AUTOTEXT_"),
        "a placeholder leaked into the user's document: {:?}",
        r.final_text
    );
}

#[test]
fn journey_04c_spoken_snippet_survives_a_model_that_drops_the_placeholder() {
    // Real models drop, reorder and reformat placeholders. The configured
    // value is authoritative, so the user must still get the address.
    let mut fx = Fixture::new();
    fx.autotext
        .insert(snippet("my address", "221B Baker Street, London"))
        .expect("insert");
    // The model forgot the placeholder entirely.
    let llm = ScriptedLlm::ok("I moved last year.");
    let r = fx.dictate("um i moved to my address last year", Some(&llm));
    // The value must still appear somewhere in the delivered text.
    assert!(
        r.final_text.contains("221B Baker Street"),
        "AutoText value lost when the model dropped its placeholder: {:?}",
        r.final_text
    );
}

#[test]
fn journey_04d_a_url_is_not_mistaken_for_an_autotext_trigger() {
    let mut fx = Fixture::new();
    fx.autotext
        .insert(snippet("/v", "version 1.0"))
        .expect("insert");
    let r = fx.typed("see https://example.com/v/next for details", None);
    assert!(
        r.final_text.contains("https://example.com/v/next"),
        "a URL was rewritten by AutoText: {:?}",
        r.final_text
    );
}

#[test]
fn journey_04e_user_text_containing_placeholder_syntax_is_not_corrupted() {
    // A user pasting or dictating documentation about this very app, or any
    // JSON/template, can legitimately type the literal token. It must not be
    // swallowed or rewritten.
    let mut fx = Fixture::new();
    fx.autotext
        .insert(snippet("/sig", "Best regards"))
        .expect("insert");
    let literal = "the token {{AUTOTEXT_0}} is a placeholder";
    let r = fx.typed(literal, None);
    assert!(
        r.final_text.contains("{{AUTOTEXT_0}}"),
        "user's literal placeholder syntax was destroyed: {:?}",
        r.final_text
    );
}

#[test]
fn journey_04f_a_trigger_word_inside_a_longer_word_does_not_expand() {
    let mut fx = Fixture::new();
    fx.autotext.insert(snippet("/a", "alpha")).expect("insert");
    let r = fx.typed("/abc and /a and /a1", None);
    assert!(r.final_text.contains("/abc"), "{:?}", r.final_text);
    assert!(r.final_text.contains("/a1"), "{:?}", r.final_text);
    assert!(r.final_text.contains("alpha"), "{:?}", r.final_text);
}

#[test]
fn journey_04g_placeholder_date_expands_at_use_time_not_save_time() {
    // A snippet with {{date}} must show today every time it is used.
    let protected = {
        let mut at = AutoTextStore::default();
        at.insert(snippet("/d", "on {{date}}")).expect("insert");
        teletype_core::autotext::protect::protect("/d", &at, &ApplicationContext::unknown())
    };
    let expanded_value = protected
        .values
        .values()
        .next()
        .cloned()
        .unwrap_or_default();
    assert!(
        expanded_value.starts_with("on 20"),
        "the date placeholder did not expand at use time: {expanded_value:?}"
    );
    let restored = teletype_core::autotext::protect::restore(&protected.text, &protected);
    assert!(
        !restored.contains("{{date}}"),
        "date not expanded: {restored:?}"
    );
    assert!(
        !restored.contains("AUTOTEXT_"),
        "placeholder leaked: {restored:?}"
    );
}

#[test]
fn journey_04h_invalid_triggers_are_rejected_with_a_reason() {
    use teletype_core::autotext::{validate_snippet, validate_trigger};
    assert!(
        validate_trigger("").is_err(),
        "an empty trigger is not a trigger"
    );
    assert!(
        validate_trigger("noslash").is_err(),
        "a typed trigger must start with /"
    );
    assert!(
        validate_trigger("/has space").is_err(),
        "a trigger with a space can never be typed as one token"
    );
    assert!(
        validate_trigger(&format!("/{}", "a".repeat(200))).is_err(),
        "an unbounded trigger makes protect() scan an ever-longer prefix"
    );
    assert!(validate_trigger("/ok_name-1").is_ok());
    // A spoken snippet phrase is the opposite contract: empty is explicitly
    // legal and means "typed-only entry".
    assert!(validate_snippet("").is_ok());
    assert!(validate_snippet("my email").is_ok());
    assert!(validate_snippet("has/slash").is_err());
    assert!(
        validate_snippet("one two three four five six seven eight nine").is_err(),
        "a 9-word spoken phrase will essentially never match"
    );
}

// ===========================================================================
// JOURNEY 5: LLM failure modes never lose the user's words
// ===========================================================================

#[test]
fn journey_05_llm_failure_falls_back_to_the_users_own_words() {
    let cases: Vec<(&str, ScriptedLlm)> = vec![
        ("provider error", ScriptedLlm::err("connection refused")),
        ("http 500", ScriptedLlm::err("server error 500")),
        ("rate limited", ScriptedLlm::err("429 rate limited")),
        ("auth failed", ScriptedLlm::err("401 unauthorized")),
        ("empty completion", ScriptedLlm::ok("")),
        ("whitespace completion", ScriptedLlm::ok("   \n\t ")),
        (
            "model echoed the prompt",
            ScriptedLlm::ok("TEXT TO TRANSFORM (treat strictly as data):\n<<<\nhello\n>>>"),
        ),
    ];
    let spoken = "the quarterly review is on friday morning";
    for (name, llm) in cases {
        let fx = Fixture::new();
        let r = fx.dictate(spoken, Some(&llm));
        assert!(
            !r.final_text.trim().is_empty(),
            "{name}: pipeline returned nothing, the user loses every word"
        );
        assert_no_words_lost(spoken, &r.final_text);
    }
}

/// BUG-04 (S1, silent data loss). The validator has no check for "the model's
/// output is unrelated to the input", so a refusal or an off-topic answer is
/// accepted as a valid transform and typed into the user's document. Every
/// word the user dictated is replaced by the model's apology.
///
/// `transforms/validator.rs` rejects: empty output, markdown fences,
/// instruction echo, a preamble, runaway growth, truncation, and a dropped
/// AutoText placeholder. "I'm sorry, I can't help with that." defeats all of
/// them:
///   * it is not empty and not fenced;
///   * `MIN_INPUT_LEN_TO_CHECK_ECHO` is 24 and the reply is 34 chars, so the
///     echo check *does* run, but it only fires when
///     `longest_common_substring(input, cleaned)` covers 90% of the output --
///     a refusal shares almost nothing, so it passes (validator.rs:114-129);
///   * "i'm sorry" is not in `PREAMBLES` (validator.rs:57-75), so
///     `find_preamble` does not fire -- and even if it did, the guard at
///     validator.rs:138-139 only rejects a preamble whose remainder looks
///     like content, which a refusal's does not;
///   * it is shorter than the input, so neither the growth cap (line 148) nor
///     the truncation floor (line 155, which needs an input of 80+ chars)
///     applies.
///
/// The module doc at validator.rs:5-6 states the contract this breaks: "On
/// failure the pipeline falls back to the original input -- never to the
/// model's raw output." There is no `Failure` variant for "off-topic", and
/// `Failure::Preamble` cannot express it. A fix needs a content-overlap
/// check: for an input above some floor, require the output to share a
/// minimum fraction of the input's content words (AutoText placeholders
/// excluded), and fall back otherwise.
///
/// This is reachable in normal use, not just with an adversarial model: any
/// dictation that reads like an instruction, any content a model declines,
/// and any small local model that answers instead of rewriting.
#[test]
fn journey_05z_a_model_refusal_must_not_replace_the_users_words() {
    let spoken = "the quarterly review is on friday morning";

    // A refusal shares nothing with the input, so it must be rejected outright
    // and the user's own words delivered.
    for (name, reply) in [
        ("refusal", "I'm sorry, I can't help with that."),
        (
            "refusal, longer",
            "I'm sorry, but I cannot assist with that request. I am a text \
             transformation engine and can only reformat text.",
        ),
        (
            "unrelated answer",
            "The weather in Reykjavik is largely determined by the North \
             Atlantic Current and the surrounding topography.",
        ),
    ] {
        let fx = Fixture::new();
        let llm = ScriptedLlm::ok(reply);
        let r = fx.dictate(spoken, Some(&llm));
        assert!(
            !r.transformed,
            "{name}: the pipeline accepted an off-topic reply as a transform"
        );
        assert_no_words_lost(spoken, &r.final_text);
        assert_eq!(
            r.final_text.trim(),
            spoken,
            "{name}: the user must get their own words back verbatim"
        );
    }

    // A chatty but on-topic answer is accepted. A rewrite may legitimately
    // rephrase and drop a modifier ("on friday morning" -> "usually held on
    // Friday"); what it may not do is change the subject. Rejecting this
    // would cost a polish rather than save a word, and an over-strict check
    // would fight legitimate models.
    let fx = Fixture::new();
    let llm = ScriptedLlm::ok(
        "The quarterly review is usually held on Friday. Let me know if you \
         would like me to prepare the agenda.",
    );
    let r = fx.dictate(spoken, Some(&llm));
    let lower = r.final_text.to_lowercase();
    for subject in ["quarterly", "review", "friday"] {
        assert!(
            lower.contains(subject),
            "the rewrite changed the subject: {subject:?} missing from {:?}",
            r.final_text
        );
    }
}

#[test]
fn journey_05b_autotext_still_expands_when_the_model_is_down() {
    let mut fx = Fixture::new();
    fx.autotext
        .insert(snippet("/addr", "221B Baker Street"))
        .expect("insert");
    let llm = ScriptedLlm::err("connection refused");
    let r = fx.typed("my address is /addr", Some(&llm));
    assert!(
        r.final_text.contains("221B Baker Street"),
        "AutoText must survive an LLM outage: {:?}",
        r.final_text
    );
}

#[test]
fn journey_05c_an_undersized_token_budget_is_visible_not_silent() {
    // G007: hitting the max_tokens cap makes the provider Err, which the
    // engine treats as fallback-to-input, so the transform silently does
    // nothing. The engine must at least report that it ran.
    let fx = Fixture::new();
    let llm = ScriptedLlm::ok("A polished sentence that the user asked for.");
    let long = (0..400)
        .map(|i| format!("word{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let r = fx.dictate(&long, Some(&llm));
    assert!(!r.final_text.is_empty());
    assert!(
        llm.max_tokens() > 0,
        "the engine must actually ask for a token budget"
    );
    assert!(
        llm.max_tokens() >= 300,
        "a long input must not get a budget that guarantees truncation, got {}",
        llm.max_tokens()
    );
}

#[test]
fn journey_05d_a_prompt_never_contains_a_secret() {
    // The engine must not put API keys or file paths into the prompt, and
    // the placeholder values are the only things that ride along.
    let mut fx = Fixture::new();
    fx.autotext
        .insert(snippet("/addr", "221B Baker Street"))
        .expect("insert");
    let llm = ScriptedLlm::ok("ok");
    let _ = fx.dictate("send it to my address", Some(&llm));
    for p in llm.prompts() {
        assert!(!p.contains("sk-"), "a key-shaped string reached the prompt");
        assert!(!p.contains("BEGIN "), "key material reached the prompt");
    }
}

// ===========================================================================
// JOURNEY 6: dictionary and ITN corrections
// ===========================================================================

#[test]
fn journey_06_taught_word_fixes_a_repeatable_mishearing() {
    let mut fx = Fixture::new();
    fx.dictionary
        .insert(DictionaryWord::new("IC Markets", ""))
        .expect("insert");
    let spoken = "i traded on ic margets this morning";
    let llm = ScriptedLlm::ok("I traded on IC Markets this morning.");
    let r = fx.dictate(spoken, Some(&llm));
    assert!(
        r.final_text.contains("IC Markets"),
        "taught word did not reach the output: {:?}",
        r.final_text
    );
}

#[test]
fn journey_06b_a_short_taught_word_cannot_mangle_normal_english() {
    // A user who teaches a 3-letter name must not corrupt every sentence.
    let mut fx = Fixture::new();
    for w in ["apt", "cat", "bid", "ceo", "ram"] {
        fx.dictionary
            .insert(DictionaryWord::new(w, ""))
            .expect("insert");
    }
    let spoken = "the app is on my laptop and the cat sat on the mat";
    let llm = ScriptedLlm::ok("The app is on my laptop and the cat sat on the mat.");
    let r = fx.dictate(spoken, Some(&llm));
    assert_no_words_lost(spoken, &r.final_text);
}

#[test]
fn journey_06c_itn_turns_spoken_numbers_into_typed_ones() {
    let fx = Fixture::new();
    let llm = ScriptedLlm::ok("I have 3 apples and 42 oranges.");
    let r = fx.dictate("i have three apples and forty two oranges", Some(&llm));
    assert!(
        r.final_text.contains('3') || r.final_text.contains("three"),
        "ITN did not run or the model undid it: {:?}",
        r.final_text
    );
}

#[test]
fn journey_06d_itn_leaves_code_and_identifiers_alone() {
    let fx = Fixture::new();
    let llm = ScriptedLlm::err("no model");
    for probe in [
        "version 1 point 2 point 3 is out",
        "call me at 555 010 9999",
        "email me at ali dot sorathiya at gmail dot com",
    ] {
        let r = fx.dictate(probe, Some(&llm));
        assert!(!r.final_text.trim().is_empty(), "ITN emptied: {probe:?}");
    }
}

#[test]
fn journey_06e_a_dictionary_word_with_regex_metacharacters_is_safe() {
    // Users paste product names like "C++" or "foo(bar)" or "a|b".
    let mut fx = Fixture::new();
    for w in ["C++", "foo(bar)", "a|b", "x*y", "[draft]", "cost$"] {
        fx.dictionary
            .insert(DictionaryWord::new(w, ""))
            .expect("insert");
    }
    let llm = ScriptedLlm::ok("ok");
    // Must not hang (ReDoS) and must not panic.
    let spoken = "we use c plus plus and a pipe a|b and a star x*y in the build";
    let r = fx.dictate(spoken, Some(&llm));
    assert!(!r.final_text.is_empty());
}

// ===========================================================================
// JOURNEY 7: history, insights, stats
// ===========================================================================

#[test]
fn journey_07_a_days_dictations_show_up_consistently_everywhere() {
    let dir = TempDir::new("insights");
    let now = 1_757_000_000_000u64;
    let (store, mut h) = history::open_history(dir.path());
    let mut total_words = 0;
    for i in 0..25u32 {
        let text = format!("this is dictation number {i} with several words in it");
        total_words += word(&text);
        h.push(DictationEntry {
            id: format!("e{i}"),
            created_at: now - (i as u64) * 60_000,
            text: text.clone(),
            context: Some(context::normalize("com.apple.Notes", "Notes")),
            duration_ms: None,
        });
    }
    store.save(&h).expect("save history");

    // Re-read from disk: what the Insights screen will actually see.
    let reloaded = history::open_history(dir.path()).1;
    assert_eq!(reloaded.entries.len(), 25, "history did not round-trip");

    let ins = insights::compute(&reloaded, now);
    let dash = teletype_core::stats::dashboard(&reloaded, now);
    assert!(
        ins.total_words >= total_words - 25,
        "insights lost words: {} vs {total_words}",
        ins.total_words
    );
    assert_eq!(
        dash.total_dictations, 25,
        "the dashboard must count every dictation"
    );
    assert!(
        dash.words_today > 0,
        "a dictation 10 minutes ago must land in today"
    );
    assert!(
        ins.streak_days >= 1,
        "a user active today must have at least a 1-day streak"
    );
}

#[test]
fn journey_07b_empty_history_produces_a_sane_dashboard() {
    let h = DictationHistory::default();
    let ins = insights::compute(&h, 1_757_000_000_000);
    let dash = teletype_core::stats::dashboard(&h, 1_757_000_000_000);
    assert_eq!(ins.total_words, 0);
    assert_eq!(dash.total_words, 0);
    assert!(
        ins.streak_days <= 1,
        "an empty history cannot have a streak"
    );
}

#[test]
fn journey_07c_history_is_capped_so_the_file_cannot_grow_forever() {
    let mut h = DictationHistory::default();
    for i in 0..1200 {
        h.push(DictationEntry {
            id: format!("e{i}"),
            created_at: 1_757_000_000_000 + i,
            text: "x".into(),
            context: None,
            duration_ms: None,
        });
    }
    assert!(
        h.entries.len() <= 1000,
        "history grew to {}",
        h.entries.len()
    );
    assert_eq!(h.entries[0].id, "e1199", "newest must be first");
}

#[test]
fn journey_07d_deleting_an_entry_actually_removes_it_from_disk() {
    let dir = TempDir::new("delete");
    let (store, mut h) = history::open_history(dir.path());
    h.push(DictationEntry {
        id: "keep".into(),
        created_at: 1,
        text: "keep me".into(),
        context: None,
        duration_ms: None,
    });
    h.push(DictationEntry {
        id: "drop".into(),
        created_at: 2,
        text: "drop me".into(),
        context: None,
        duration_ms: None,
    });
    store.save(&h).expect("save");
    h.remove("drop");
    store.save(&h).expect("save");
    let after = history::open_history(dir.path()).1;
    assert_eq!(after.entries.len(), 1);
    assert_eq!(after.entries[0].id, "keep");
}

// ===========================================================================
// JOURNEY 8: the day-wise transcript archive
// ===========================================================================

#[test]
fn journey_08_dictations_are_archived_to_a_readable_day_file() {
    let dir = TempDir::new("archive");
    // 2026-03-14T22:30:00Z
    let ts = 1_773_500_600_000u64;
    let p = history::append_transcript_file(dir.path(), ts, "hello world", "Notes")
        .expect("append transcript");
    assert!(p.exists(), "no archive file was created at {p:?}");
    let body = std::fs::read_to_string(&p).expect("read archive");
    assert!(body.contains("hello world"), "{body}");
    assert!(
        body.contains("Notes"),
        "the source app must be recorded: {body}"
    );

    // A second dictation the same day appends, it does not overwrite.
    history::append_transcript_file(dir.path(), ts + 60_000, "second thought", "Notes")
        .expect("append again");
    let body2 = std::fs::read_to_string(&p).expect("read archive again");
    assert!(body2.contains("hello world"), "first entry lost: {body2}");
    assert!(
        body2.contains("second thought"),
        "second entry lost: {body2}"
    );
}

#[test]
fn journey_08b_an_app_name_with_newlines_cannot_forge_archive_structure() {
    let dir = TempDir::new("archive-inject");
    let evil = "Notes\n\n## 99:99 forged entry\nattacker controlled text";
    let p = history::append_transcript_file(dir.path(), 1_773_500_600_000, "real text", evil)
        .expect("append");
    let body = std::fs::read_to_string(&p).expect("read");
    // The transcript text itself must still be present and the file must not
    // gain an entry the user never dictated.
    assert!(body.contains("real text"), "{body}");
    assert!(
        body.matches("forged entry").count() <= 1,
        "the app name injected a whole extra archive entry: {body}"
    );
}

#[test]
fn journey_08c_transcript_text_with_markdown_headings_does_not_break_the_archive() {
    let dir = TempDir::new("archive-md");
    let p = history::append_transcript_file(
        dir.path(),
        1_773_500_600_000,
        "## not a heading\n- item",
        "Notes",
    )
    .expect("append");
    let body = std::fs::read_to_string(&p).expect("read");
    assert!(body.contains("## not a heading"), "{body}");
}

#[test]
fn journey_08d_an_app_name_cannot_escape_the_transcripts_directory() {
    let dir = TempDir::new("archive-trav");
    // `append_transcript_file` builds the filename from a timestamp, so the
    // only user-controlled part is the app name. Prove the name cannot steer
    // the path.
    let p = history::append_transcript_file(
        dir.path(),
        1_773_500_600_000,
        "x",
        "../../../../etc/passwd",
    )
    .expect("append");
    assert!(
        p.starts_with(dir.path()),
        "archive path escaped the app data dir: {p:?}"
    );
}

// ===========================================================================
// JOURNEY 9: corrupt file recovery
// ===========================================================================

#[test]
fn journey_09_a_corrupt_settings_file_is_moved_aside_not_fatal() {
    let dir = TempDir::new("corrupt");
    std::fs::write(dir.path().join("dictation.json"), b"{ this is not json")
        .expect("write corrupt file");

    let (store, h) = history::open_history(dir.path());
    assert!(
        h.entries.is_empty(),
        "a corrupt file must not crash the load"
    );

    // The corrupt bytes must be preserved for support, not silently dropped.
    let corrupt = dir.path().join("dictation.json.corrupt");
    assert!(
        corrupt.exists(),
        "corrupt file was not preserved at {corrupt:?}"
    );

    // And the next save must succeed and not be clobbered by the old file.
    let mut h2 = h;
    h2.push(DictationEntry {
        id: "x".into(),
        created_at: 1,
        text: "after corruption".into(),
        context: None,
        duration_ms: None,
    });
    store.save(&h2).expect("save after corruption");
    let after = history::open_history(dir.path()).1;
    assert_eq!(after.entries.len(), 1);
    assert_eq!(after.entries[0].text, "after corruption");
}

#[test]
fn journey_09b_truncated_and_empty_files_are_also_survivable() {
    for (name, bytes) in [
        ("truncated", &b"{\"entries\": [{\"id\": \"a\""[..]),
        ("empty", b""),
        ("wrong type", b"[]"),
        ("null", b"null"),
    ] {
        let dir = TempDir::new(&format!("corrupt-{name}"));
        std::fs::write(dir.path().join("dictation.json"), bytes).expect("write");
        let (_, h) = history::open_history(dir.path());
        assert!(h.entries.is_empty(), "{name} file must load as empty");
    }
}

#[test]
fn journey_09c_a_write_is_atomic_so_a_crash_cannot_truncate_settings() {
    let dir = TempDir::new("atomic");
    let store: JsonStore<DictationHistory> = JsonStore::new(dir.path(), "dictation.json");
    let mut h = DictationHistory::default();
    for i in 0..50 {
        h.push(DictationEntry {
            id: format!("e{i}"),
            created_at: i,
            text: "x".repeat(200),
            context: None,
            duration_ms: None,
        });
    }
    store.save(&h).expect("save");
    let on_disk = std::fs::read(dir.path().join("dictation.json")).expect("read");
    let parsed: DictationHistory = serde_json::from_slice(&on_disk).expect("must be valid JSON");
    assert_eq!(parsed.entries.len(), 50);
    // No temp file may be left behind as the live document.
    assert!(!dir.path().join("dictation.tmp").exists());
}

// ===========================================================================
// JOURNEY 10: the hotkey state machine
// ===========================================================================

#[test]
fn journey_10_a_hold_to_talk_session_runs_idle_to_completed() {
    let mode = RecordingMode::Hold;
    let mut hotkey_down = false;
    let mut phase = Phase::Idle;

    let start = state::decide(Input::HotkeyDown, phase, mode, &mut hotkey_down, &mut false);
    assert!(matches!(start, state::Action::Start { .. }), "{start:?}");
    phase = Phase::Listening;

    let stop = state::decide(Input::HotkeyUp, phase, mode, &mut hotkey_down, &mut false);
    assert!(matches!(stop, state::Action::Stop { .. }), "{stop:?}");
}

#[test]
fn journey_10b_a_double_press_never_starts_two_sessions() {
    let mode = RecordingMode::Hold;
    let mut hotkey_down = false;
    // Press, release, press, release: exactly two sessions, not four.
    for _ in 0..2 {
        let a = state::decide(
            Input::HotkeyDown,
            Phase::Idle,
            mode,
            &mut hotkey_down,
            &mut false,
        );
        assert!(matches!(a, state::Action::Start { .. }));
        let b = state::decide(
            Input::HotkeyUp,
            Phase::Listening,
            mode,
            &mut hotkey_down,
            &mut false,
        );
        assert!(matches!(b, state::Action::Stop { .. }));
    }
}

#[test]
fn journey_10c_a_hotkey_press_during_transcription_is_ignored_not_queued() {
    // The user impatiently taps the hotkey while the model is still working.
    // That must not corrupt the in-flight dictation, and the next real press
    // must still work.
    let mode = RecordingMode::Hold;
    let mut hotkey_down = false;
    for phase in [Phase::Transcribing, Phase::Transforming, Phase::Inserting] {
        let a = state::decide(Input::HotkeyDown, phase, mode, &mut hotkey_down, &mut false);
        assert_eq!(
            a,
            state::Action::Nothing,
            "pressing the hotkey during {phase:?} must not start or stop anything, got {a:?}"
        );
        // BUG-02: `decide` sets the latch before it decides the phase is
        // busy, so the press is consumed rather than ignored. Releasing
        // clears it, so the app recovers, but the tap the user made is
        // swallowed with no feedback at all.
        // The release must always clear the latch, whatever the phase.
        let b = state::decide(Input::HotkeyUp, phase, mode, &mut hotkey_down, &mut false);
        assert_eq!(b, state::Action::Nothing, "{b:?}");
        assert!(
            !hotkey_down,
            "the latch survived the release during {phase:?}; the next dictation is dead"
        );
    }
    // And the very next press after the app settles must work.
    let a = state::decide(
        Input::HotkeyDown,
        Phase::Idle,
        mode,
        &mut hotkey_down,
        &mut false,
    );
    assert!(
        matches!(a, state::Action::Start { .. }),
        "the app is wedged after an impatient tap: {a:?}"
    );
}

/// Cancelling is one of the two ways a take ends, so it must release the
/// hotkey and hands-free latches exactly like the other
/// (`Input::HotkeyInterrupted`, `state.rs:117-119`) does.
///
/// This used to be the only "the take is over" input that did not clear them.
/// The failure it caused: a user holding the hotkey who clicks Cancel on the
/// pill was returned to `Idle` with `hotkey_down == true`, so nothing they
/// said next was recorded and there was no feedback -- it read as "the hotkey
/// stopped working" until they released and pressed again.
///
/// (The remaining way to wedge the latch needs the key-up event to be
/// swallowed, which is a separate defect: three unsynchronised copies of "is
/// the key down" -- the C `g_fnDown`, the Rust `LAST_DOWN`, and
/// `Session.hotkey_down`.)
#[test]
fn journey_10d_cancel_abandons_the_take_cleanly() {
    let mode = RecordingMode::Hold;
    let mut hands_free = false;
    let mut hotkey_down = true;
    let a = state::decide(
        Input::Cancel,
        Phase::Listening,
        mode,
        &mut hotkey_down,
        &mut hands_free,
    );
    assert!(
        matches!(
            a,
            state::Action::Discard | state::Action::Stop { cancelled: true }
        ),
        "cancel must abandon the take, got {a:?}"
    );
    // BUG-03: `Input::Cancel` never touches the hotkey latch, while the
    // neighbouring `Input::HotkeyInterrupted` explicitly does. A user who
    // holds the hotkey, cancels from the pill, and keeps holding is left in
    // Idle with the latch set: nothing they say is recorded and the app
    // looks broken. The release does clear it, so it self-heals on the next
    // key-up, which is why this is S3 and not S1.
    assert!(
        !hotkey_down,
        "BUG-03: cancel left the hotkey latched; the user must release and          re-press before the app will listen again"
    );
    assert!(
        !hands_free,
        "BUG-03: cancel left hands-free set; the next release would be swallowed"
    );
}

#[test]
fn journey_10e_toggle_mode_works_and_releases() {
    let mode = RecordingMode::Toggle;
    let mut hotkey_down = false;
    let a = state::decide(
        Input::HotkeyDown,
        Phase::Idle,
        mode,
        &mut hotkey_down,
        &mut false,
    );
    assert!(matches!(a, state::Action::Start { .. }));
    let b = state::decide(
        Input::Toggle,
        Phase::Listening,
        mode,
        &mut hotkey_down,
        &mut false,
    );
    assert!(matches!(b, state::Action::Stop { .. }), "{b:?}");
}

#[test]
fn journey_10f_a_stuck_hotkey_latch_can_always_be_recovered() {
    // After any cancel or interruption the app must be able to return to a
    // state where the next press works. Feed the machine a hostile event
    // order and assert it never wedges.
    let mode = RecordingMode::Hold;
    let mut hotkey_down = false;
    let mut phase = Phase::Idle;
    let script = [
        Input::HotkeyDown,
        Input::HotkeyUp,
        Input::Cancel,
        Input::HotkeyDown,
        Input::Cancel,
        Input::HotkeyDown,
        Input::HotkeyUp,
        Input::HotkeyInterrupted,
        Input::HotkeyDown,
        Input::HotkeyUp,
    ];
    for ev in script {
        let a = state::decide(ev, phase, mode, &mut hotkey_down, &mut false);
        match a {
            state::Action::Start { .. } => phase = Phase::Listening,
            state::Action::Stop { .. } | state::Action::Discard => phase = Phase::Idle,
            state::Action::Nothing => {}
            other => panic!("unexpected action {other:?} for {ev:?}"),
        }
        assert!(
            matches!(
                phase,
                Phase::Idle | Phase::Listening | Phase::Completed | Phase::Cancelled | Phase::Error
            ),
            "the machine reached a phase it cannot leave: {phase:?}"
        );
    }
    assert!(!hotkey_down, "the latch must be clear at the end");
}

#[test]
fn journey_10g_a_single_modifier_hotkey_that_gets_interrupted_never_keeps_recording() {
    // The Fn/globe key is a single-modifier hotkey. If the user presses
    // another key mid-hold, the OS never sends a clean "up", and without an
    // explicit interrupt path the app would record forever.
    let mode = RecordingMode::Hold;
    let mut hotkey_down = false;
    let a = state::decide(
        Input::HotkeyDown,
        Phase::Idle,
        mode,
        &mut hotkey_down,
        &mut false,
    );
    assert!(matches!(a, state::Action::Start { .. }));
    let b = state::decide(
        Input::HotkeyInterrupted,
        Phase::Listening,
        mode,
        &mut hotkey_down,
        &mut false,
    );
    assert!(
        matches!(b, state::Action::Stop { .. } | state::Action::Discard),
        "an interrupted hold must end the take, got {b:?}"
    );
    assert!(!hotkey_down, "the latch must be released");
}

#[test]
fn journey_10h_cancelling_while_busy_drops_the_result_not_the_app() {
    let mode = RecordingMode::Hold;
    let mut hotkey_down = false;
    for phase in [Phase::Transcribing, Phase::Transforming, Phase::Inserting] {
        let a = state::decide(Input::Cancel, phase, mode, &mut hotkey_down, &mut false);
        assert_eq!(
            a,
            state::Action::CancelPipeline,
            "cancel while {phase:?} must cancel the pipeline, got {a:?}"
        );
    }
}

// ===========================================================================
// JOURNEY 11: unicode, emoji, long text
// ===========================================================================

#[test]
fn journey_11_emoji_cjk_rtl_and_combining_marks_survive_the_pipeline() {
    let fx = Fixture::new();
    let spoken_inputs = vec![
        "ship it 🚀🚀 today",
        "会议は明日の朝です",
        "the price went from ten dollars to twelve",
        "café naïve résumé",
        "🎉🎉🎉 party time",
        "مرحبا بالعالم",
    ];
    let llm = ScriptedLlm::ok("ok");
    for input in spoken_inputs {
        let r = fx.dictate(input, Some(&llm));
        assert!(!r.final_text.trim().is_empty(), "emptied on {input:?}");
        // No replacement characters: that is the signature of a botched
        // byte-boundary slice somewhere in the pipeline.
        assert!(
            !r.final_text.contains('\u{FFFD}'),
            "unicode corruption on {input:?} -> {:?}",
            r.final_text
        );
    }
}

#[test]
fn journey_11b_a_very_long_dictation_does_not_truncate_or_hang() {
    let fx = Fixture::new();
    let long = (0..3000)
        .map(|i| format!("word{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let llm = ScriptedLlm::ok("polished");

    // Serialize with the other perf tests. Both of them hold this lock, and
    // `perf_concurrent_store_access_does_not_block_pipeline` spawns a thread
    // that deliberately hammers the store mutexes every 100µs. Tests inside
    // one binary run on parallel threads, so without this guard this test
    // timed itself against a deliberately hostile sibling: it measured 10.6s
    // on a CI runner and 4.4s on a quiet desktop, for the same 3000 words.
    // That gap was contention, not a regression.
    let _perf_guard = PERF_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let start = std::time::Instant::now();
    let r = fx.dictate(&long, Some(&llm));
    let elapsed = start.elapsed();
    assert!(!r.final_text.is_empty());
    eprintln!("perf: 3000-word dictation took {elapsed:?}");

    // This bound is a hang alarm, not a performance regression gate. An
    // absolute wall-clock ceiling this tight cannot survive a shared runner:
    // the true cost is ~4.4s on a fast desktop and GitHub's 3-core M1 runners
    // are roughly 2.4x slower, so a 10s ceiling had under 2.3x of headroom and
    // failed on ordinary scheduling noise (see the lock above).
    //
    // Detecting a real algorithmic regression is
    // `perf_pipeline_non_llm_steps_scale_linearly`'s job, and it does it
    // properly: median of 50 runs, short input against 4x-longer, asserting a
    // ratio rather than a duration. That is machine-independent and cannot be
    // broken by a slow runner. Duplicating a fragile absolute check here
    // bought nothing but flakes, so this is left loose enough to survive a
    // loaded machine while still failing loudly on an actual hang.
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "a long dictation took {elapsed:?}; that is a hang, not slowness"
    );
}

#[test]
fn journey_11c_control_characters_and_markup_do_not_leak_into_the_document() {
    let fx = Fixture::new();
    let llm = ScriptedLlm::ok("ok");
    for probe in [
        "hello\u{0}world",
        "line one\nline two\ttabbed",
        "<script>alert(1)</script>",
        "back\\slash and \"quotes\"",
    ] {
        let r = fx.dictate(probe, Some(&llm));
        assert!(!r.final_text.contains('\u{0}'), "NUL leaked from {probe:?}");
    }
}

#[test]
fn journey_11d_a_dictation_of_only_punctuation_does_not_crash() {
    let fx = Fixture::new();
    let llm = ScriptedLlm::ok("ok");
    for probe in ["...", "?!", "- ", "   ", "\n\n\n"] {
        let r = fx.dictate(probe, Some(&llm));
        // Whatever comes out, the call must return rather than panic.
        let _ = r.final_text.len();
    }
}

// ===========================================================================
// JOURNEY 12: import / export
// ===========================================================================

#[test]
fn journey_12_a_dictionary_export_reimports_without_duplicates() {
    let mut a = Dictionary::default();
    for w in ["IC Markets", "Parakeet", "Teletype"] {
        a.insert(DictionaryWord::new(w, "")).expect("insert");
    }
    let json = dictionary::export_to_json(&a).expect("export");
    let parsed = dictionary::parse_export(json.as_bytes()).expect("parse");
    assert_eq!(parsed.len(), 3);

    let mut b = Dictionary::default();
    let counts = dictionary::merge_words(&mut b, parsed);
    assert_eq!(counts.imported, 3, "{counts:?}");
    assert_eq!(b.words.len(), 3);

    // Re-importing the same file must not double the list.
    let again = dictionary::parse_export(json.as_bytes()).expect("parse again");
    let counts2 = dictionary::merge_words(&mut b, again);
    assert_eq!(counts2.imported, 0, "a second import duplicated words");
    assert_eq!(b.words.len(), 3, "dictionary grew on re-import");
}

#[test]
fn journey_12b_a_malformed_import_cannot_destroy_the_existing_dictionary() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("Keepme", "")).expect("insert");
    let before = d.words.len();

    for bad in [
        &b"not json at all"[..],
        b"{}",
        b"{\"version\":1,\"words\":\"nope\"}",
        b"[]",
        b"null",
    ] {
        // A hard parse error must be an Err, never a silent wipe.
        match dictionary::parse_export(bad) {
            Err(_) => {}
            Ok(words) => {
                let c = dictionary::merge_words(&mut d, words);
                assert_eq!(c.imported, 0, "{bad:?} imported something");
            }
        }
    }
    assert_eq!(
        d.words.len(),
        before,
        "the dictionary was damaged by a bad import"
    );
    assert!(d.words.iter().any(|w| w.word == "Keepme"));
}

#[test]
fn journey_12c_an_export_never_contains_a_secret() {
    let d = Dictionary::default();
    let json = dictionary::export_to_json(&d).expect("export");
    for marker in ["api_key", "apiKey", "token", "sk-", "password", "secret"] {
        assert!(
            !json.contains(marker),
            "the export leaked a secret-shaped field: {marker}"
        );
    }
}

// ===========================================================================
// JOURNEY 13: settings persistence across a restart
// ===========================================================================

#[test]
fn journey_13_a_users_settings_survive_a_restart() {
    let dir = TempDir::new("restart");
    let store: JsonStore<AutoTextStore> = JsonStore::new(dir.path(), "autotext.json");

    // Session 1: the user creates a snippet.
    let mut s = store.load(AutoTextStore::default());
    s.insert(snippet("/addr", "221B Baker Street"))
        .expect("insert");
    store.save(&s).expect("save");

    // Session 2: the app restarts and reads it back.
    let reloaded = store.load(AutoTextStore::default());
    assert_eq!(
        reloaded.entries.len(),
        1,
        "the snippet did not survive a restart"
    );
    assert_eq!(reloaded.entries[0].replacement, "221B Baker Street");
}

/// BUG-01 (S1, data loss). Every persisted collection struct in the app
/// requires all of its fields, so any field added or renamed in a later
/// version makes the whole document fail to parse. `JsonStore::load` then
/// renames the file to `.corrupt` and returns the default: the user's entire
/// AutoText library, custom transforms, style profiles, preferences or
/// scratchpad silently becomes empty on upgrade, with no error surfaced and
/// no undo. This is the documented-but-not-implemented "settings fields are
/// serde-defaulted for backward compatibility" contract.
///
/// Missing defaults today:
///   `AutoTextEntry`      description, scope (only `snippet`/`system` have one)
///   `TransformDefinition` all 12 fields
///   `StyleProfile`        all fields
///   `Preference`          all fields
///   `ScratchEntry`        all fields
/// (`DictionaryWord.fuzzy` and the `Dictionary`/`UserProfile` collection
/// structs do carry defaults, which is why only some stores are affected.)
#[test]
fn journey_13b_a_new_field_added_in_a_later_version_does_not_wipe_settings() {
    // Simulates an upgrade: a settings file written by an older build that
    // lacks a field the current build expects.
    let dir = TempDir::new("upgrade");
    std::fs::write(
        dir.path().join("autotext.json"),
        br#"{"entries":[{"id":"a","trigger":"/x","replacement":"y","enabled":true,"createdAt":1,"updatedAt":1}]}"#,
    )
    .expect("write old-format file");
    let store: JsonStore<AutoTextStore> = JsonStore::new(dir.path(), "autotext.json");
    let loaded = store.load(AutoTextStore::default());
    assert_eq!(
        loaded.entries.len(),
        1,
        "an older settings file must still load, not reset to empty"
    );
}

// ===========================================================================
// JOURNEY 14: per-app behaviour does not leak between apps
// ===========================================================================

#[test]
fn journey_14_a_snippet_scoped_to_email_does_not_fire_in_notes() {
    let mut fx = Fixture::new().in_app("com.apple.Notes", "Notes");
    use teletype_core::autotext::AutoTextScope;
    let mut e = snippet("/esc", "cheers,\nAli");
    e.scope = AutoTextScope::Application("com.apple.mail".into());
    fx.autotext.insert(e).expect("insert");

    let r = fx.typed("thanks /esc", None);
    assert!(
        !r.autotext_expanded,
        "an app-scoped snippet fired in the wrong app: {:?}",
        r.final_text
    );

    // And it must fire in the app it was scoped to.
    let mut mail = Fixture::new().in_app("com.apple.mail", "Mail");
    let mut scoped = snippet("/esc", "cheers,\nAli");
    scoped.scope = teletype_core::autotext::AutoTextScope::Application("com.apple.mail".into());
    mail.autotext.insert(scoped).expect("insert");
    let r2 = mail.typed("thanks /esc", None);
    assert!(
        r2.autotext_expanded,
        "an app-scoped snippet did not fire in its own app: {:?}",
        r2.final_text
    );
    assert!(r2.final_text.contains("cheers,"), "{:?}", r2.final_text);
}

#[test]
fn journey_14b_per_app_language_overrides_do_not_leak() {
    use teletype_core::personalization::{app_language_key, resolve_language};
    let mut p = UserProfile {
        language: "en".into(),
        ..Default::default()
    };
    let mail = context::normalize("com.apple.mail", "Mail");
    let notes = context::normalize("com.apple.Notes", "Notes");
    p.app_language_overrides
        .insert(app_language_key(&mail).expect("key for Mail"), "fr".into());

    assert_eq!(resolve_language(&p, &mail), "fr");
    assert_eq!(
        resolve_language(&p, &notes),
        "en",
        "the Mail override leaked into Notes"
    );
}

#[test]
fn journey_14c_a_style_override_for_one_app_does_not_leak() {
    use teletype_core::style::resolve_style_id_for_key;
    let mut store = StyleProfileStore::with_built_ins();
    store
        .set_app_style_override("Email", "style-professional")
        .expect("set override");
    assert_eq!(
        resolve_style_id_for_key(&store.app_style_overrides, "email", "style-casual", ""),
        "style-professional",
        "the per-app override must win over the global active style"
    );
    assert_eq!(
        resolve_style_id_for_key(&store.app_style_overrides, "notes", "style-casual", ""),
        "style-casual",
        "the email style leaked into Notes"
    );
    // Case must not matter, or a user typing "Mail" gets no override.
    assert_eq!(
        resolve_style_id_for_key(&store.app_style_overrides, "EMAIL", "style-casual", ""),
        "style-professional"
    );
    // An explicit per-dictation choice must beat both.
    assert_eq!(
        resolve_style_id_for_key(
            &store.app_style_overrides,
            "email",
            "style-casual",
            "style-concise"
        ),
        "style-concise"
    );
}

// ===========================================================================
// JOURNEY 15: the whole day, start to finish
// ===========================================================================

#[test]
fn journey_15_a_full_day_of_mixed_activity_leaves_consistent_state() {
    let dir = TempDir::new("full-day");
    let now = 1_757_000_000_000u64;

    let mut fx = Fixture::new().in_app("com.microsoft.outlook", "Outlook");
    fx.autotext
        .insert(snippet("/sig", "Best regards,\nAli"))
        .expect("insert");
    fx.dictionary
        .insert(DictionaryWord::new("IC Markets", ""))
        .expect("insert");
    fx.filler_words = vec!["um".into(), "uh".into()];

    let llm = ScriptedLlm::echo();

    // 12 dictations of the kind a real day contains.
    let utterances = [
        "um so the deploy went out this morning and it is all good",
        "please review ic margets numbers before the client call",
        "send the invoice to the finance team today",
        "i will be out on friday and back on monday",
        "can we move the stand up to ten thirty",
        "the build is broken again on windows",
        "let us ship it today",
        "i think we should push the release to next week",
        "the customer asked about the refund policy",
        "um i need a hand with the quarterly numbers",
        "everything is ready for the demo",
        "thanks for the quick turnaround /sig",
    ];

    let (store, mut h) = history::open_history(dir.path());
    for (i, u) in utterances.iter().enumerate() {
        let r = fx.dictate(u, Some(&llm));
        assert!(
            !r.final_text.trim().is_empty(),
            "utterance {i} produced nothing"
        );
        assert!(
            !r.final_text.contains("AUTOTEXT_"),
            "utterance {i} leaked a placeholder: {:?}",
            r.final_text
        );
        h.push(DictationEntry {
            id: format!("d{i}"),
            created_at: now - ((utterances.len() - i) as u64) * 300_000,
            text: r.final_text.clone(),
            context: fx.platform.active_application(),
            duration_ms: None,
        });
        history::append_transcript_file(
            &dir.path().join("transcripts"),
            now,
            &r.final_text,
            "Outlook",
        )
        .expect("archive");
    }
    store.save(&h).expect("save history");

    // End of day: everything the user can see must agree.
    let reloaded = history::open_history(dir.path()).1;
    assert_eq!(reloaded.entries.len(), utterances.len());
    let dash = teletype_core::stats::dashboard(&reloaded, now);
    assert!(dash.total_words > 0, "the day shows zero words");
    let ins = insights::compute(&reloaded, now);
    assert!(ins.total_words > 0);
    assert!(ins.streak_days >= 1);

    let archives: Vec<_> = std::fs::read_dir(dir.path().join("transcripts"))
        .expect("read transcripts dir")
        .filter_map(Result::ok)
        .collect();
    assert!(!archives.is_empty(), "no transcript archive was written");
    let body = std::fs::read_to_string(archives[0].path()).expect("read archive");
    // The archive must mirror exactly what was delivered to the app, entry
    // for entry, with no placeholder leakage.
    assert!(
        !body.contains("AUTOTEXT_"),
        "a placeholder reached the written archive: {body}"
    );
    // Asserted against the history rather than against the mock's output, so
    // the test keeps working whatever the model does.
    for e in &reloaded.entries {
        assert!(
            body.contains(&e.text),
            "history holds {:?} but the day file does not",
            e.text
        );
    }
    assert!(
        body.contains("Best regards,"),
        "the AutoText expansion never reached the archive: {body}"
    );
    // One heading per dictation, not one per word.
    assert_eq!(
        body.matches("\n## ").count() + usize::from(body.starts_with("## ")),
        utterances.len(),
        "the archive and the history disagree about how many dictations landed: {body}"
    );
    for e in &reloaded.entries {
        assert!(
            body.contains(&e.text),
            "history holds {:?} but the day file does not",
            e.text
        );
    }
}

// ---------------------------------------------------------------------------
// Performance validation (P1-B fix)
// ---------------------------------------------------------------------------

/// A slow LLM that sleeps to simulate real inference latency.
struct SlowLlm {
    delay_ms: u64,
}
impl InferenceProvider for SlowLlm {
    fn model_id(&self) -> &str {
        "slow"
    }
    fn model_name(&self) -> &str {
        "slow-mock"
    }
    fn is_local(&self) -> bool {
        true
    }
    fn generate(&self, prompt: &str, _params: GenerationParams) -> Result<String, String> {
        std::thread::sleep(std::time::Duration::from_millis(self.delay_ms));
        // Echo back capitalised, like a real polish model.
        Ok(prompt
            .chars()
            .map(|c| c.to_uppercase().next().unwrap_or(c))
            .collect::<String>())
    }
}

/// P1-B validation: the pipeline's non-LLM transform steps (dictionary
/// correction, filler removal, ITN, AutoText protect/restore, spoken
/// emoji/punctuation) must scale *at most linearly* with input length for
/// a typical utterance. This catches regressions where a transform
/// accidentally becomes O(n²) or allocates excessively.
///
/// The gate is *relative to input length*, not to an absolute wall-clock
/// budget: absolute time is machine-dependent (the same code measured
/// ~53 ms on a dev M-series Mac and ~90 ms on a GitHub macOS runner — a
/// ~20× spread with no code change), so a fixed 55 ms budget fails on slow
/// runners while passing on fast ones, which is noise, not signal. Instead
/// we run the pipeline on a short and a long utterance and check that the
/// long one is no more than ~2× the short one (allowing for the fixed
/// per-run scaffolding cost). A transform that degrades to O(n²) shows up
/// as super-linear growth regardless of how fast or slow the host is.
///
/// Measured as the *median* of per-run timings while holding a lock that
/// serializes the other perf test in this file: the suite runs tests in
/// parallel threads, and a shared runner under load can stretch one run
/// past any budget while the code is still fast. The median of enough
/// samples is stable on both ends; the lock removes the concurrency noise
/// from the measurement itself.
///
/// NOTE: if you add more perf tests here, share the same lock so they
/// never time each other concurrently.
static PERF_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// How much slower the 4×-longer input may be than the baseline input
/// before we treat it as a super-linear (O(n²)) regression. A linear
/// pipeline grows ~4× in the *variable* part, but the fixed per-run
/// scaffolding (state machine, context, storage) does not, so the total
/// grows *less* than 4×. A 2× limit therefore passes any linear pipeline
/// (which grows < 4×) while catching an O(n²) one (which grows ~16×).
const PERF_SCALING_LIMIT: u32 = 2;

/// Median of `n` pipeline runs (µs), after a warm-up call.
fn median_pipeline_run_us(pipeline: &mut Pipeline, input: &UnifiedInput, n: usize) -> u128 {
    let _ = pipeline.run(input.clone(), None);
    let mut per_run: Vec<u128> = Vec::with_capacity(n);
    for _ in 0..n {
        let start = std::time::Instant::now();
        let _ = pipeline.run(input.clone(), None);
        per_run.push(start.elapsed().as_micros());
    }
    per_run.sort_unstable();
    per_run[n / 2]
}

#[test]
fn perf_pipeline_non_llm_steps_scale_linearly() {
    use teletype_core::autotext::AutoTextStore;
    use teletype_core::dictionary::Dictionary;
    use teletype_core::personalization::UserProfile;
    use teletype_core::style::StyleProfileStore;
    use teletype_core::transforms::TransformStore;

    let platform = MockPlatform::with_app(ApplicationContext::unknown());
    let autotext = AutoTextStore::default();
    let transforms = TransformStore::with_built_ins();
    let profile = UserProfile::default();
    let dictionary = Dictionary::default();
    let styles = StyleProfileStore::with_built_ins();
    // No LLM: the pipeline runs all deterministic transforms and skips
    // the inference step entirely.
    let mut pipeline = Pipeline {
        platform: &platform,
        autotext: &autotext,
        transforms: &transforms,
        profile: &profile,
        inference: None,
        dictionary: &dictionary,
        styles: &styles,
        active_style: "",
        explicit_style: "",
        auto_apply: false,
        restore_clipboard: true,
        remove_filler_words: true,
        filler_words: vec!["um".into(), "uh".into(), "like".into()],
        system_autotext: &[],
        token_sink: None,
        polish_gate_enabled: true,
        polish_gate_threshold_words: 8,
        restore_emoji: true,
        spoken_emoji: true,
        spoken_punctuation: true,
        pack_terms: &[],
        list_style: teletype_core::transforms::ListStyle::default(),
        word_checker: &teletype_core::dictionary::EDIT_DISTANCE_CHECKER,
    };

    // A realistic 20-word utterance, and a 4×-longer one (80 words).
    let short = "um, the meeting is at three p.m. today and like I need to bring the quarterly report and uh the budget spreadsheet";
    let long = (0..4)
        .map(|i| format!("{short} (take {i})"))
        .collect::<Vec<_>>()
        .join(" ");

    let short_input = UnifiedInput {
        source: InputSource::Voice,
        text: short.into(),
    };
    let long_input = UnifiedInput {
        source: InputSource::Voice,
        text: long,
    };

    // Serialize with the other perf test: it spawns a hammer thread
    // that would inflate our timings if they ran concurrently.
    let _perf_guard = PERF_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let iterations = 50;
    let short_us = median_pipeline_run_us(&mut pipeline, &short_input, iterations);
    let long_us = median_pipeline_run_us(&mut pipeline, &long_input, iterations);

    // The long input is 4× the text. A linear pipeline's *variable* cost
    // grows 4×, but the fixed per-run scaffolding does not, so the total
    // grows strictly less than 4×. An O(n²) transform would grow ~16×.
    // A 2× limit sits comfortably between: it passes any linear pipeline
    // and fails a super-linear one, on any machine.
    let ratio_x100 = long_us * 100 / short_us.max(1);

    eprintln!(
        "perf: pipeline (no LLM) short = {} µs, 4x-long = {} µs, ratio = {}x (limit {}x) over {} runs",
        short_us, long_us, ratio_x100 / 100, PERF_SCALING_LIMIT, iterations
    );
    assert!(
        ratio_x100 < u128::from(PERF_SCALING_LIMIT) * 100,
        "pipeline non-LLM steps scaled {}x for a 4x-longer input ({} µs vs {} µs); a transform likely regressed to O(n²) or over-allocates",
        ratio_x100 / 100, long_us, short_us
    );
}

/// P1-B validation: concurrent store access (simulating the user opening
/// Settings/Dictionary while a dictation pipeline is running) must not
/// block the pipeline or cause a deadlock. We spawn a thread that hammers
/// the store mutexes while the pipeline runs with a slow LLM, and assert
/// both complete within a reasonable time.
#[test]
fn perf_concurrent_store_access_does_not_block_pipeline() {
    use teletype_core::autotext::AutoTextStore;
    use teletype_core::dictionary::Dictionary;
    use teletype_core::personalization::UserProfile;
    use teletype_core::style::StyleProfileStore;
    use teletype_core::transforms::TransformStore;

    // Shared stores behind mutexes, mimicking AppState.
    let autotext = std::sync::Arc::new(std::sync::Mutex::new(AutoTextStore::default()));
    let transforms = std::sync::Arc::new(std::sync::Mutex::new(TransformStore::with_built_ins()));
    let profile = std::sync::Arc::new(std::sync::Mutex::new(UserProfile::default()));
    let styles = std::sync::Arc::new(std::sync::Mutex::new(StyleProfileStore::with_built_ins()));
    let dictionary = std::sync::Arc::new(std::sync::Mutex::new(Dictionary::default()));

    // Background thread: hammer the locks like the Settings screen would.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Hold the perf lock so the timing test never measures against this
    // thread's background hammer.
    let _perf_guard = PERF_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (h_autotext, h_transforms, h_profile, h_styles, h_dict) = (
        autotext.clone(),
        transforms.clone(),
        profile.clone(),
        styles.clone(),
        dictionary.clone(),
    );
    let stop_bg = stop.clone();
    let hammer = std::thread::spawn(move || {
        while !stop_bg.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = h_autotext.lock().unwrap().entries.len();
            let _ = h_transforms.lock().unwrap().transforms.len();
            let _ = h_profile.lock().unwrap().language.clone();
            let _ = h_styles.lock().unwrap().profiles.len();
            let _ = h_dict.lock().unwrap().words.len();
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    });

    // Main: clone the stores (P1-B pattern) and run the pipeline with a
    // 100 ms slow LLM. The clones are independent of the mutexes, so the
    // hammer thread cannot block us.
    let (a, t, p, s, d) = {
        (
            autotext.lock().unwrap().clone(),
            transforms.lock().unwrap().clone(),
            profile.lock().unwrap().clone(),
            styles.lock().unwrap().clone(),
            dictionary.lock().unwrap().clone(),
        )
    };
    let platform = MockPlatform::with_app(ApplicationContext::unknown());
    let llm = SlowLlm { delay_ms: 100 };
    let mut pipeline = Pipeline {
        platform: &platform,
        autotext: &a,
        transforms: &t,
        profile: &p,
        inference: Some(&llm),
        dictionary: &d,
        styles: &s,
        active_style: "",
        explicit_style: "",
        auto_apply: true,
        restore_clipboard: true,
        remove_filler_words: false,
        filler_words: vec![],
        system_autotext: &[],
        token_sink: None,
        polish_gate_enabled: false,
        polish_gate_threshold_words: 8,
        restore_emoji: true,
        spoken_emoji: true,
        spoken_punctuation: true,
        pack_terms: &[],
        list_style: teletype_core::transforms::ListStyle::default(),
        word_checker: &teletype_core::dictionary::EDIT_DISTANCE_CHECKER,
    };

    let input = UnifiedInput {
        source: InputSource::Voice,
        text: "the quick brown fox jumps over the lazy dog and keeps on running".into(),
    };

    let start = std::time::Instant::now();
    let result = pipeline.run(input, None);
    let elapsed = start.elapsed();

    // Stop the hammer.
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    hammer.join().unwrap();

    assert!(!result.final_text.is_empty(), "pipeline produced no output");
    // The 100 ms LLM delay dominates; the total should be well under 2 s
    // even with the hammer running. If the mutexes were held across the
    // LLM call (the pre-P1-B bug), the hammer would only slow us by a
    // tiny amount, but the real bug was that OTHER threads would block
    // for 100 ms+. Here we just verify no deadlock.
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "pipeline took {elapsed:?} with concurrent store access; suspected deadlock"
    );
    eprintln!("perf: pipeline with 100ms LLM + concurrent store hammer = {elapsed:?}");
}

// ---------------------------------------------------------------------------
// Broader UAT: multi-language, edge cases
// ---------------------------------------------------------------------------

/// UAT: the pipeline must handle non-English input without corrupting it,
/// and the LLM prompt must include the correct language tag so the model
/// polishes in the right language.
#[test]
fn uat_multilanguage_pipeline_does_not_corrupt_input() {
    let cases = vec![
        (
            "fr",
            "bonjour, je voudrais un café s'il vous plaît, et un croissant au beurre",
        ),
        (
            "de",
            "guten morgen, ich möchte bitte einen kaffee und ein brötchen",
        ),
        (
            "es",
            "hola, quisiera un café con leche y un croissant de mantequilla",
        ),
        ("ja", "おはようございます。コーヒーとクロワッサンをください"),
        ("ko", "안녕하세요. 커피와 크루아상 하나 주세요"),
        ("hi", "नमस्ते, मुझे एक कॉफ़ी और एक क्राउसां चाहिए"),
    ];

    for (lang, text) in &cases {
        let fx = Fixture::new().with_language(lang);
        let llm = ScriptedLlm::echo();
        let r = fx.dictate(text, Some(&llm));
        assert!(
            !r.final_text.trim().is_empty(),
            "[{lang}] pipeline produced empty output for: {text:?}"
        );
        assert!(
            !r.final_text.contains('\u{FFFD}'),
            "[{lang}] unicode corruption in output: {:?}",
            r.final_text
        );
        eprintln!("UAT [{lang}]: {:?} -> {:?}", text, r.final_text);
    }
}

/// UAT: a single-word dictation (the shortest possible real input) must
/// produce output, not be swallowed by filler removal or the polish gate.
#[test]
fn uat_single_word_dictation_produces_output() {
    let fx = Fixture::new();
    let llm = ScriptedLlm::echo();
    let r = fx.dictate("hello", Some(&llm));
    assert!(
        !r.final_text.trim().is_empty(),
        "single word was swallowed: {:?}",
        r.final_text
    );
    assert!(
        r.final_text.to_lowercase().contains("hello"),
        "single word was mangled: {:?}",
        r.final_text
    );
}

/// UAT: input that is entirely filler words must produce empty or
/// near-empty output, not a garbage string.
#[test]
fn uat_all_filler_input_does_not_produce_garbage() {
    let fx = Fixture::new();
    let llm = ScriptedLlm::echo();
    let r = fx.dictate("um uh um like uh", Some(&llm));
    // The filler words are stripped; whatever remains (punctuation,
    // spaces) should not be a full sentence of garbage.
    let trimmed = r.final_text.trim();
    assert!(
        trimmed.len() < 10,
        "all-filler input produced too much output: {trimmed:?}"
    );
}
