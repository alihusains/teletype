use teletype_catalog::schema::{Artifact, Capability, Purpose, SpeechEntry};
use teletype_catalog::validate::*;

fn base_speech(id: &str) -> SpeechEntry {
    SpeechEntry {
        id: id.into(),
        name: format!("Model {id}"),
        engine: "whisper".into(),
        capabilities: vec![Capability::Asr],
        languages: Some(vec!["en".into(), "de".into()]),
        english_only: false,
        artifacts: vec![Artifact {
            file: "a.gguf".into(),
            bytes: 1_000_000,
            sha256: Some("a".repeat(64)),
        }],
        base_url: None,
        size_mb: 1,
        min_ram_gb: 2,
        speed: 5.0,
        accuracy: 5.0,
        recommended: true,
        auxiliary: false,
        runs_on: vec!["macos".into()],
        purposes: vec![Purpose {
            label: "Multilingual".into(),
            basis: "inferred".into(),
            evidence: None,
        }],
        description: "d".into(),
    }
}

#[test]
fn a_good_speech_set_passes() {
    assert!(validate_speech(&[base_speech("a")]).is_ok());
}

#[test]
fn empty_domain_is_valid() {
    assert!(validate_speech(&[]).is_ok());
}

#[test]
fn duplicate_ids_fail() {
    let mut b = base_speech("a");
    b.recommended = false;
    assert!(validate_speech(&[base_speech("a"), b]).is_err());
}

#[test]
fn zero_recommended_in_nonempty_speech_fails() {
    let mut e = base_speech("a");
    e.recommended = false;
    assert!(validate_speech(&[e]).is_err());
}

#[test]
fn multiple_recommended_is_allowed() {
    assert!(validate_speech(&[base_speech("a"), base_speech("b")]).is_ok());
}

#[test]
fn unknown_engine_fails() {
    let mut e = base_speech("a");
    e.engine = "nope".into();
    assert!(validate_speech(&[e]).is_err());
}

#[test]
fn bad_purpose_label_fails() {
    let mut e = base_speech("a");
    e.purposes = vec![Purpose {
        label: "Made-up".into(),
        basis: "inferred".into(),
        evidence: None,
    }];
    assert!(validate_speech(&[e]).is_err());
}

#[test]
fn english_only_with_other_languages_fails() {
    let mut e = base_speech("a");
    e.english_only = true;
    e.languages = Some(vec!["en".into(), "de".into()]);
    assert!(validate_speech(&[e]).is_err());
}

#[test]
fn size_mb_below_artifact_sum_fails() {
    let mut e = base_speech("a");
    e.size_mb = 0;
    assert!(validate_speech(&[e]).is_err());
}
