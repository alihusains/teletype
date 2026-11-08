use teletype_catalog::Catalog;

#[test]
fn bundled_catalog_loads_with_populated_speech_rows() {
    let c = Catalog::bundled();
    assert!(c.speech_by_id("parakeet-tdt-v3").is_some());
    assert_eq!(c.speech().len(), 23);
}

#[test]
fn load_from_missing_dir_is_an_error() {
    assert!(Catalog::load_from_dir(std::path::Path::new("/nonexistent/teletype-catalog")).is_err());
}
