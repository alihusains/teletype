use teletype_catalog::Catalog;

#[test]
fn llm_json_has_the_polish_rows() {
    let c = Catalog::bundled();
    for id in [
        "s1-mini",
        "qwen3.5-2b",
        "speakoflow-mini",
        "qwen2.5-3b",
        "fast",
        "qwen3-0.6b",
        "quality",
        "qwen2.5-7b",
        "gemma2-2b",
    ] {
        let e = c
            .llm_by_id(id)
            .unwrap_or_else(|| panic!("missing llm row {id}"));
        assert!(
            e.capabilities
                .contains(&teletype_catalog::Capability::Polish),
            "{id}"
        );
    }
    assert!(c.llm_by_id("neudecide-q4").is_none());
}

#[test]
fn command_json_has_the_decider_with_display_fields() {
    let c = Catalog::bundled();
    let d = c
        .command_by_id("neudecide-q4")
        .unwrap_or_else(|| panic!("missing neudecide-q4"));
    assert!(d
        .capabilities
        .contains(&teletype_catalog::Capability::ToolCalling));
    assert_eq!(d.engine, "onnx");
    assert_eq!(d.shards.len(), 6);
    assert!(
        !d.specialty.is_empty(),
        "decider display fields must survive (R3)"
    );
}

#[test]
fn bundled_catalog_is_complete() {
    let c = Catalog::bundled();
    assert_eq!(c.speech().len(), 22);
    assert_eq!(c.llm().len(), 9);
    assert_eq!(c.command().len(), 1);
}
