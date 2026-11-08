use teletype_catalog::gate::*;
use teletype_catalog::schema::{Capability, LlmEntry, SpeechEntry};

// Build minimal fixtures from the schema (fill every required field; no todo!()).
fn llm(tool_calling: bool) -> LlmEntry {
    let caps = if tool_calling {
        vec![Capability::ToolCalling]
    } else {
        vec![Capability::Polish]
    };
    LlmEntry {
        id: "l".into(),
        name: "L".into(),
        size_label: "1 GB".into(),
        capabilities: caps,
        url: "https://example.invalid/l".into(),
        size_mb: 1,
        description: "d".into(),
        specialty: "cleanup".into(),
        sha256: None,
        backup_url: None,
        attribution: None,
        license_name: None,
        license_url: None,
        requires_license_accept: false,
        recommended: true,
        runs_on_cpu: true,
        verdict: "recommended".into(),
        verdict_note: String::new(),
        sort_order: 1,
        shards: vec![],
        runs_on: vec!["macos".into()],
    }
}

fn asr() -> SpeechEntry {
    SpeechEntry {
        id: "a".into(),
        name: "A".into(),
        engine: "whisper".into(),
        capabilities: vec![Capability::Asr],
        languages: Some(vec!["en".into()]),
        english_only: true,
        artifacts: vec![],
        base_url: None,
        size_mb: 1,
        min_ram_gb: 1,
        speed: 5.0,
        accuracy: 5.0,
        recommended: true,
        auxiliary: false,
        runs_on: vec!["macos".into()],
        purposes: vec![],
        description: "d".into(),
    }
}

#[test]
fn gate_is_the_conjunction_of_the_two_parts() {
    assert!(matches!(
        command_mode_available(&llm(true), &asr(), true),
        CommandModeStatus::Available
    ));
    assert!(matches!(
        command_mode_available(&llm(false), &asr(), true),
        CommandModeStatus::MissingToolCallingLlm
    ));
    assert!(matches!(
        command_mode_available(&llm(true), &asr(), false),
        CommandModeStatus::MissingUsableAsr
    ));
    assert!(matches!(
        command_mode_available(&llm(false), &asr(), false),
        CommandModeStatus::MissingBoth
    ));
}
