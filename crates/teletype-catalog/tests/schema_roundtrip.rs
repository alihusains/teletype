use teletype_catalog::schema::{Capability, SpeechEntry};

#[test]
fn speech_entry_roundtrips_through_json() {
    let json = r#"{
      "id": "qwen3-asr", "name": "Qwen3-ASR 1.7B", "engine": "llama-server",
      "capabilities": ["asr"], "languages": ["en", "de", "pl"], "englishOnly": false,
      "artifacts": [
        {"file": "Qwen3-ASR-1.7B-Q8_0.gguf", "bytes": 2165034944,
         "sha256": "58e22d0532d4eacaf034cfac17a6fed159f37c41390c710186783be439d1fc57"},
        {"file": "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf", "bytes": 355709344,
         "sha256": "46c1d533af3f354ceb37ce855dbceff7da7fa7cf1e6a523df3b13440bd164c0d"}
      ],
      "baseUrl": "https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main",
      "sizeMb": 2521, "minRamGb": 4, "speed": 6.0, "accuracy": 9.0,
      "recommended": false, "auxiliary": false, "runsOn": ["macos"],
      "purposes": [{"label": "Multilingual", "basis": "inferred", "evidence": null}],
      "description": "..."
    }"#;
    let e: SpeechEntry = serde_json::from_str(json).unwrap();
    assert_eq!(e.engine, "llama-server");
    assert!(e.capabilities.contains(&Capability::Asr));
    assert_eq!(e.artifacts.len(), 2);
    assert_eq!(e.artifacts[0].bytes, 2_165_034_944);
    let back = serde_json::to_string(&e).unwrap();
    let again: SpeechEntry = serde_json::from_str(&back).unwrap();
    assert_eq!(again.id, e.id);
}
