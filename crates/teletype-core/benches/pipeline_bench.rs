//! Pipeline bench: `Pipeline::run` with dictionaries of 0, 500 and 5,000 entries.
//!
//! NOTE on the plan (§P0.3 says `Pipeline::process`): the current API is
//! `Pipeline::run(input, explicit_transform)`; this bench uses `run` with no
//! model loaded, which exercises the dictionary-correction + ITN + AutoText
//! path (the deterministic prefix every voice dictation pays for).
//!
//! The input sentence contains near-miss tokens (`kubernates`, `telatype`)
//! so the fuzzy tiers actually scan the dictionary instead of short-circuiting
//! on exact matches.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use teletype_core::{
    autotext::AutoTextStore,
    context::ApplicationContext,
    dictionary::{Dictionary, DictionaryWord, EDIT_DISTANCE_CHECKER},
    personalization::UserProfile,
    pipeline::{InputSource, Pipeline, UnifiedInput},
    platform::MockPlatform,
    style::StyleProfileStore,
    transforms::{ListStyle, TransformStore},
};

const INPUT: &str = "the kubernates cluster needs a telatype update before the deploy goes out \
    and it is all good for the release tomorrow morning";

fn build_dict(n: usize) -> Dictionary {
    let mut d = Dictionary::default();
    for i in 0..n {
        // Unique padded names; every insert sorts, so build time is O(n^2)
        // but that is setup cost outside the measured loop.
        let w = DictionaryWord::new(format!("Benchterm{i:05}"), "");
        d.insert(w).expect("unique bench terms");
    }
    // A couple of real targets so the fuzzy path has something to hit.
    for w in ["Kubernetes", "Teletype"] {
        if d.find(w).is_none() {
            d.insert(DictionaryWord::new(w, "")).expect("seed target");
        }
    }
    d
}

fn run_with_dict(dict: &Dictionary) -> String {
    let platform = MockPlatform::with_app(ApplicationContext::unknown());
    let autotext = AutoTextStore::default();
    let transforms = TransformStore::with_built_ins();
    let profile = UserProfile::default();
    let styles = StyleProfileStore::with_built_ins();
    let mut pipeline = Pipeline {
        platform: &platform,
        autotext: &autotext,
        transforms: &transforms,
        profile: &profile,
        inference: None,
        decision: None,
        dictionary: dict,
        word_checker: &EDIT_DISTANCE_CHECKER,
        styles: &styles,
        active_style: "",
        explicit_style: "",
        auto_apply: false,
        restore_clipboard: false,
        remove_filler_words: false,
        filler_words: vec![],
        restore_emoji: false,
        spoken_emoji: false,
        spoken_punctuation: false,
        system_autotext: &[],
        token_sink: None,
        polish_gate_enabled: false,
        polish_gate_threshold_words: 8,
        pack_terms: &[],
        list_style: ListStyle::default(),
    };
    pipeline
        .run(
            UnifiedInput {
                source: InputSource::Voice,
                text: INPUT.into(),
            },
            None,
        )
        .final_text
}

fn pipeline_dict_sizes(c: &mut Criterion) {
    // Sanity: the pipeline actually corrects the near-misses.
    let small = build_dict(0);
    let out = run_with_dict(&small);
    assert!(
        out.contains("Kubernetes") || out.contains("kubernates"),
        "pipeline bench produced no output: {out}"
    );

    let mut group = c.benchmark_group("pipeline_dict");
    for &n in &[0usize, 500, 5000] {
        let dict = build_dict(n);
        group.bench_function(format!("entries_{n}"), |b| {
            b.iter(|| black_box(run_with_dict(black_box(&dict))))
        });
    }
    group.finish();
}

criterion_group!(benches, pipeline_dict_sizes);
criterion_main!(benches);
