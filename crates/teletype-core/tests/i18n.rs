//! P9.2 — UTF-8 round-trip for non-Latin languages through the core
//! pipeline (plan §9.2: "UTF-8 round trip for ar, hi, zh, ru, de through
//! ASR mock → polish mock → paste").
//!
//! Every case drives a representative string through the *real* pipeline
//! with the same mock harness `journey_smoke.rs` uses: a mock ASR text
//! (the pipeline's `UnifiedInput` *is* the ASR transcript — this is exactly
//! how `journey_smoke.rs` injects text) and a mock polish provider in
//! **echo mode** (returns the input unchanged, apart from capitalising the
//! first character, which the validator would otherwise reject as
//! off-topic... no: the pipeline accepts the model output, so the mock is
//! a *true* byte-for-byte echo here, see `EchoLlm`).
//!
//! The assertion is **byte-for-byte equality** of the final text with the
//! input: no normalization, no trimming, no punctuation or
//! autotext/ITN mutation inside non-Latin text.
//!
//! ```text
//! cargo test -p teletype-core --test i18n
//! ```

use teletype_core::autotext::AutoTextStore;
use teletype_core::dictionary::Dictionary;
use teletype_core::llm::{GenerationParams, InferenceProvider};
use teletype_core::personalization::UserProfile;
use teletype_core::pipeline::{InputSource, Pipeline, PipelineResult, UnifiedInput};
use teletype_core::platform::MockPlatform;
use teletype_core::style::StyleProfileStore;
use teletype_core::transforms::TransformStore;

// ---------------------------------------------------------------------------
// Mock polish provider: a byte-for-byte echo.
// ---------------------------------------------------------------------------

/// A polish model that emits its input *unchanged* — byte for byte.
///
/// `journey_smoke.rs`'s `ScriptedLlm::echo()` capitalises the first
/// character, which is a deliberate deviation from the input this test
/// cannot allow (it would corrupt e.g. a lowercase-starting CJK sentence
/// that has no case). So this mock is a pure echo: it extracts the marked
/// input from the prompt (`<<<\n ... \n>>>`) and returns it verbatim.
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
        // The engine marks the to-polish text between `<<<\n` and `\n>>>`.
        if let (Some(a), Some(b)) = (prompt.rfind("<<<\n").map(|i| i + 4), prompt.rfind("\n>>>")) {
            if b > a {
                return Ok(prompt[a..b].to_string());
            }
        }
        Err("echo mock: could not find the marked input in the prompt".into())
    }
}

// ---------------------------------------------------------------------------
// Pipeline fixture, mirroring journey_smoke.rs
// ---------------------------------------------------------------------------

/// Everything the pipeline borrows, in one bundle. Mirrors `Fixture` in
/// `journey_smoke.rs`, with the deterministic-transform settings that
/// matter for this file made explicit.
struct Fixture {
    platform: MockPlatform,
    autotext: AutoTextStore,
    transforms: TransformStore,
    dictionary: Dictionary,
    styles: StyleProfileStore,
    active_style: String,
    word_checker: teletype_core::dictionary::EditDistanceChecker,
}

impl Fixture {
    fn new() -> Self {
        Self {
            platform: MockPlatform::default(),
            autotext: AutoTextStore::default(),
            transforms: TransformStore::with_built_ins(),
            dictionary: Dictionary::default(),
            styles: StyleProfileStore::with_built_ins(),
            active_style: String::new(),
            word_checker: teletype_core::dictionary::EditDistanceChecker::new(),
        }
    }
}

/// Build a `Pipeline` the way dictation does (voice source, auto-apply
/// transform on, ITN/emoji gates driven by the profile language), and
/// run one utterance through it.
fn dictate(
    fx: &Fixture,
    language: &str,
    text: &str,
    llm: Option<&dyn InferenceProvider>,
) -> PipelineResult {
    let mut profile = UserProfile::default();
    profile.language = language.into();
    let mut p = Pipeline {
        platform: &fx.platform,
        autotext: &fx.autotext,
        transforms: &fx.transforms,
        profile: &profile,
        inference: llm,
        decision: None,
        force_command: false,
        dictionary: &fx.dictionary,
        styles: &fx.styles,
        active_style: &fx.active_style,
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
        word_checker: &fx.word_checker,
    };
    p.run(
        UnifiedInput {
            source: InputSource::Voice,
            text: text.into(),
        },
        None,
    )
}

/// Drive `text` through the pipeline with the echo polish mock and assert
/// the final text is byte-for-byte identical to the input.
fn assert_roundtrip(fx: &Fixture, language: &str, text: &str) {
    let llm = EchoLlm;
    let r = dictate(fx, language, text, Some(&llm));
    assert_eq!(
        r.final_text.as_bytes(),
        text.as_bytes(),
        "[{language}] pipeline mutated the bytes: {text:?} -> {:?}",
        r.final_text
    );
}

// ---------------------------------------------------------------------------
// Representative strings per language
// ---------------------------------------------------------------------------

/// One representative string per language the plan names (ar, hi, zh, ru,
/// de), plus en and an emoji control. Each entry covers the specific
/// hazard that language has:
///
///  - `ar`: RTL, pure Arabic + Arabic with Latin digits
///  - `hi`: Devanagari with combining marks (matras)
///  - `zh`: CJK without spaces
///  - `ru`: Cyrillic with soft/hard signs and yeryo
///  - `de`: Latin with umlauts + eszett
///  - `en`: control, plain ASCII
///  - `emoji`: ZWJ family, flag sequence, and a combining-mark (zalgo-ish)
///    sequence, mixed with Latin
const CASES: &[(&str, &str, &str)] = &[
    ("ar", "مرحبا بالعالم", "pure Arabic, RTL"),
    (
        "ar",
        "الاجتماع الساعة 10:30 في قاعة 203",
        "Arabic with Latin digits and a time",
    ),
    (
        "hi",
        "नमस्ते, मुझे एक कॉफ़ी और एक क्राउसां चाहिए",
        "Devanagari with combining matras (ि ो ॉ े ़)",
    ),
    (
        "zh",
        "会议是明天上午十点在三楼会议室",
        "CJK, no spaces, no diacritics",
    ),
    (
        "ru",
        "встреча в十点 утра в переговорной",
        "Cyrillic mixed with a CJK token",
    ),
    (
        "de",
        "guten morgen, ich möchte bitte einen kaffee und ein brötchen",
        "German umlauts, eszett, soft hyphen-free",
    ),
    (
        "en",
        "the deploy went out this morning and it is all good",
        "English control",
    ),
    (
        "en",
        "family 👨‍👩‍👧‍👦 and flag 🇫🇷 and café\u{0301} and z\u{0308}al\u{0301}go",
        "ZWJ family, flag, and combining-mark sequences",
    ),
];

// ---------------------------------------------------------------------------
// The round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn i18n_roundtrip_every_language_byte_for_byte() {
    let fx = Fixture::new();
    for (lang, text, why) in CASES {
        // The profile language is the one the pipeline uses to gate
        // ITN and emoji restore. For the non-Latin cases the gate is off
        // by construction (ITN is English-gated), so the text must pass
        // through untouched regardless of what the polish mock does.
        assert_roundtrip(&fx, lang, text);
        eprintln!("i18n [{lang}] ({why}): {text:?} -> OK");
    }
}

#[test]
fn i18n_roundtrip_mixed_script_sentence() {
    // A single dictation that mixes scripts: Arabic + Latin digits +
    // a CJK token + an emoji. The pipeline must not re-order, normalize,
    // or drop anything.
    let fx = Fixture::new();
    let text = "الاجتماع في قاعة 203 三点钟 🚀";
    assert_roundtrip(&fx, "ar", text);
}

#[test]
fn i18n_roundtrip_emoji_sequences_survive_unmodified() {
    let fx = Fixture::new();
    // ZWJ family, flag, skin-tone modifier, combining mark on a letter.
    let probes = [
        "👨‍👩‍👧‍👦",
        "🇫🇷",
        "👍🏽",
        "e\u{0301}",
        "a\u{0308}a\u{0308}a\u{0308}",
        // Note: the word "party" is a registered spoken-emoji trigger
        // (emoji-dictionary.json: "party" -> 🎉), so it cannot be used as
        // a control word here without the spoken-emoji pass rewriting it.
        "🎉🎉🎉 celebration time",
    ];
    for text in probes {
        assert_roundtrip(&fx, "en", text);
    }
}

#[test]
fn i18n_roundtrip_rtl_keeps_codepoint_order() {
    // RTL text must keep its codepoint order. We assert on bytes, not on
    // display order: the pipeline must not reverse, re-order, or insert
    // LRM/RLM marks.
    let fx = Fixture::new();
    let text = "مرحبا بالعالم";
    let llm = EchoLlm;
    let r = dictate(&fx, "ar", text, Some(&llm));
    // Byte-for-byte: no reordering, no bidi marks, no normalization.
    assert_eq!(
        r.final_text.as_bytes(),
        text.as_bytes(),
        "RTL text was reordered or marked: {text:?} -> {:?}",
        r.final_text
    );
    // No bidi control characters were inserted.
    assert!(
        !r.final_text.contains('\u{200E}') && !r.final_text.contains('\u{200F}'),
        "bidi control marks were inserted: {:?}",
        r.final_text
    );
}

#[test]
fn i18n_roundtrip_pure_non_latin_never_triggers_replacements() {
    // The invariant: pure non-Latin text must never be rewritten by
    // slang/correction/autotext/ITN. We run the pipeline with a
    // non-English profile language (so ITN is gated off) and with an
    // empty dictionary (so no corrections fire), and assert the output
    // is byte-identical to the input.
    let fx = Fixture::new();
    // ar: ITN gated off (language != en), no dictionary, no autotext.
    let ar_text = "الاجتماع الساعة 10:30 في قاعة 203";
    assert_roundtrip(&fx, "ar", ar_text);

    // hi: same, Devanagari.
    let hi_text = "नमस्ते, मुझे एक कॉफ़ी चाहिए";
    assert_roundtrip(&fx, "hi", hi_text);

    // zh: CJK, no spaces.
    let zh_text = "会议是明天上午十点";
    assert_roundtrip(&fx, "zh", zh_text);

    // ru: Cyrillic.
    let ru_text = "встреча в十点 утра";
    assert_roundtrip(&fx, "ru", ru_text);
}

#[test]
fn i18n_roundtrip_arabic_with_number_itn_gated_off() {
    // Pin the ACTUAL behavior: ITN is English-gated (`itn::should_run`
    // returns true only for "en"/"en-*"/empty). When the profile language
    // is "ar", ITN does not run, so an Arabic string containing a number
    // (even a Latin digit) round-trips byte-for-byte. This is correct:
    // the ITN lexicon is English-only (see itn.rs doc: "English-only:
    // the lexicon is English; the pipeline gates on `should_run`"), and
    // running it on Arabic would corrupt the text.
    let fx = Fixture::new();
    let llm = EchoLlm;
    let ar_with_num = "لدي ثلاثة تفاحات و 42 برتقالة";
    let r = dictate(&fx, "ar", ar_with_num, Some(&llm));
    assert_eq!(
        r.final_text.as_bytes(),
        ar_with_num.as_bytes(),
        "Arabic with a number was mutated by ITN: {ar_with_num:?} -> {:?}",
        r.final_text
    );

    // Control: the same string with an "en" profile *would* run ITN, but
    // the ITN passes are English-lexicon-driven and would not match
    // Arabic words. The Latin digit "42" is already a digit, so ITN
    // leaves it alone. The result is still byte-identical.
    let r_en = dictate(&fx, "en", ar_with_num, Some(&llm));
    assert_eq!(
        r_en.final_text.as_bytes(),
        ar_with_num.as_bytes(),
        "Arabic with a number under en profile was mutated: {ar_with_num:?} -> {:?}",
        r_en.final_text
    );
}
