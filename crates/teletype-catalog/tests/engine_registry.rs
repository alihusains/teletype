use teletype_catalog::engine::Engine;

#[test]
fn known_names_resolve_to_engines() {
    assert_eq!(Engine::from_name("llama-server").unwrap(), Engine::Qwen3Asr);
    assert_eq!(Engine::from_name("whisper").unwrap(), Engine::Whisper);
    assert_eq!(
        Engine::from_name("parakeet-coreml").unwrap(),
        Engine::ParakeetCoreml
    );
    assert_eq!(Engine::from_name("onnx").unwrap(), Engine::Onnx);
}

#[test]
fn unknown_name_is_an_error_not_a_variant() {
    assert!(Engine::from_name("no-such-engine").is_err());
    assert!(Engine::from_name("").is_err());
}

#[test]
fn name_roundtrips() {
    for (name, e) in [
        ("whisper", Engine::Whisper),
        ("llama-server", Engine::Qwen3Asr),
    ] {
        assert_eq!(Engine::from_name(name).unwrap(), e);
        assert_eq!(Engine::from_name(name).unwrap().name(), name);
    }
}
