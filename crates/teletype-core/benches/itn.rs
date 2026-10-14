//! Criterion benchmarks for the ITN normalizer.
//!
//! Run with: `cargo bench -p teletype-core --bench itn`
//! Compare runs with: `cargo bench -p teletype-core --bench itn -- --save-baseline before`
//! then after your change: `cargo bench -p teletype-core --bench itn -- --baseline before`

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use teletype_core::itn::normalize;

/// Representative corpus of utterances that exercise every major ITN pass.
///
/// Sourced from the eval corpus and hand-crafted to cover: cardinals, years,
/// dates, times, phone runs, decimals, money, percentages, ordinals, mixed
/// magnitude, URLs, and plain prose (no ITN needed).
static CORPUS: &[(&str, &str)] = &[
    // cardinals
    ("twenty twenty six", "2026"),
    ("three hundred and forty five thousand", "345,000"),
    ("eighty million dollars", "$80 million"),
    // dates + times
    ("march fifteenth twenty twenty five", "March 15th, 2025"),
    ("three fifteen pm", "3:15 PM"),
    // phone
    ("two oh three nine five four oh six", "203-954-0006"),
    // ordinals
    ("the thirty second president", "the 32nd president"),
    // decimals + money
    ("three point one four", "3.14"),
    ("forty nine ninety nine", "$49.99"),
    // percent
    ("twelve point five percent", "12.5%"),
    // URL / email (should pass through unchanged)
    ("www dot example dot com", "www.example.com"),
    // plain prose (no-op pass — checks that ITN doesn't corrupt normal text)
    (
        "the quarterly report is ready to send",
        "the quarterly report is ready to send",
    ),
    // mixed long utterance exercising many passes
    (
        "on january first twenty twenty four we raised two million five hundred \
         thousand dollars at a valuation of twelve point five million",
        "",
    ),
];

fn bench_normalize_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("itn/normalize");
    for (input, _) in CORPUS {
        group.bench_with_input(
            BenchmarkId::new("single", truncate_label(input, 40)),
            input,
            |b, s| b.iter(|| normalize(criterion::black_box(s))),
        );
    }
    group.finish();
}

fn bench_normalize_corpus(c: &mut Criterion) {
    c.bench_function("itn/normalize/corpus_all", |b| {
        b.iter(|| {
            for (input, _) in CORPUS {
                criterion::black_box(normalize(criterion::black_box(input)));
            }
        })
    });
}

/// First-call cost includes any one-time `LazyLock` initialization. This bench
/// is deliberately separate so we can confirm that the lock pays its setup cost
/// only once (subsequent calls should be on par with `bench_normalize_single`).
fn bench_normalize_first_call(c: &mut Criterion) {
    // Call once to ensure the lock is warm before the actual timing loop —
    // this measures the "already initialized" steady-state, not cold start.
    let _ = normalize("zero");
    c.bench_function("itn/normalize/steady_state_zero", |b| {
        b.iter(|| normalize(criterion::black_box("zero")))
    });
}

fn truncate_label(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Truncate at a character boundary.
        let boundary = s
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= max)
            .last()
            .unwrap_or(max);
        &s[..boundary]
    }
}

criterion_group!(
    benches,
    bench_normalize_single,
    bench_normalize_corpus,
    bench_normalize_first_call
);
criterion_main!(benches);
