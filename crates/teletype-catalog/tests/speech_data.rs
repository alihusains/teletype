use teletype_catalog::Catalog;

#[test]
fn speech_json_has_all_22_models() {
    let c = Catalog::bundled();
    let expected = [
        "parakeet-tdt-v3",
        "fluid-speech-mini",
        "parakeet-tdt-v3-q4",
        "parakeet-tdt-v3-f16",
        "parakeet-ultra-coreml",
        "parakeet-v2-coreml",
        "parakeet-v3-coreml",
        "parakeet-ctc-110m-coreml",
        "sherpa-parakeet-v3-int8",
        "sherpa-parakeet-v2-int8",
        "large-v3-turbo-q5",
        "large-v3-turbo",
        "small-en",
        "base-en",
        "base",
        "tiny",
        "qwen3-asr",
        "apple-speech-transcriber",
        "apple-dictation-transcriber",
        "nemotron-3.5-asr-streaming-q4",
        "moonshine-streaming-medium-q8",
        "granite-speech-470m-q4",
    ];
    assert_eq!(c.speech().len(), expected.len());
    for id in expected {
        assert!(c.speech_by_id(id).is_some(), "missing speech row {id}");
    }
    assert!(
        c.speech_by_id("ivy").is_none(),
        "ivy is added in Task 13, not migrated"
    );
}

#[test]
fn every_speech_row_has_asr_capability_and_a_known_engine() {
    let c = Catalog::bundled();
    for e in c.speech() {
        assert!(
            e.capabilities.contains(&teletype_catalog::Capability::Asr),
            "{}",
            e.id
        );
        let _ = teletype_catalog::engine::Engine::from_name(&e.engine).unwrap();
    }
}

#[test]
fn qwen3_asr_row_carries_its_two_artifacts_and_corrected_size() {
    let c = Catalog::bundled();
    let q = c.speech_by_id("qwen3-asr").unwrap();
    assert_eq!(q.artifacts.len(), 2);
    assert_eq!(q.size_mb, 2521);
}
