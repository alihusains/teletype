use super::language_mismatch_notice;
use crate::dictation::SpeechEngineKind;
use teletype_speech::catalog::find;

#[test]
fn flags_locked_non_english_on_english_only_engine() {
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
