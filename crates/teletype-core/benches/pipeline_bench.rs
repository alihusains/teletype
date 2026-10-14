use criterion::{black_box, criterion_group, criterion_main, Criterion};
use teletype_core::pipeline::{InputSource, Pipeline, UnifiedInput};
use teletype_core::platform::MockPlatform;
use teletype_core::autotext::AutoTextStore;
use teletype_core::transforms::TransformStore;
use teletype_core::personalization::UserProfile;
use teletype_core::dictionary::{Dictionary, EDIT_DISTANCE_CHECKER};
use teletype_core::style::StyleProfileStore;
use teletype_core::vocab::PackTerm;

fn bench_pipeline_small(c: &mut Criterion) {
    let platform = MockPlatform::with_app(Default::default());
    let autotext = AutoTextStore::default();
    let transforms = TransformStore::default();
    let profile = UserProfile::default();
    let dictionary = Dictionary::default();
    let styles = StyleProfileStore::with_built_ins();
    let pack_terms: Vec<PackTerm> = Vec::new();
    let filler_words: Vec<String> = vec![];
    let mut p = Pipeline {
        platform: &platform,
        autotext: &autotext,
        transforms: &transforms,
        profile: &profile,
        inference: None,
        dictionary: &dictionary,
        word_checker: &EDIT_DISTANCE_CHECKER,
        styles: &styles,
        active_style: "default",
        explicit_style: "",
        auto_apply: false,
        restore_clipboard: true,
        remove_filler_words: false,
        filler_words: filler_words.clone(),
        restore_emoji: true,
        spoken_emoji: true,
        spoken_punctuation: true,
        system_autotext: &[],
        token_sink: None,
        polish_gate_enabled: false,
        polish_gate_threshold_words: 8,
        pack_terms: &pack_terms,
        list_style: teletype_core::transforms::ListStyle::Bullets,
    };
    let inputs = vec![
        "hello world this is a test",
        "i need to buy milk eggs and bread",
        "the meeting is at three pm tomorrow",
    ];
    c.bench_function("pipeline_small", |b| {
        b.iter(|| {
            for s in &inputs {
                let _ = p.run(
                    black_box(UnifiedInput {
                        source: InputSource::Voice,
                        text: s.to_string(),
                    }),
                    None,
                );
            }
        });
    });
}

criterion_group!(benches, bench_pipeline_small);
criterion_main!(benches);
