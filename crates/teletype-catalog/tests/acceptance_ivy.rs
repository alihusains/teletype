use teletype_catalog::Catalog;

#[test]
fn ivy_flows_through_every_layer_from_pure_data() {
    let c = Catalog::bundled();
    let ivy = c.speech_by_id("ivy").expect("ivy must be in the data");
    assert_eq!(
        teletype_catalog::engine::Engine::from_name(&ivy.engine).unwrap(),
        teletype_catalog::engine::Engine::Qwen3Asr
    );
    let models_dir = std::path::Path::new("/models");
    assert_eq!(
        c.speech_provider_path(ivy, models_dir),
        models_dir.join("ivy")
    );
    assert_eq!(ivy.artifacts.len(), 2);
    assert_eq!(
        ivy.artifacts[0].sha256.as_deref(),
        Some("da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed")
    );
}
