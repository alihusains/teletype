use criterion::{black_box, criterion_group, criterion_main, Criterion};
use teletype_core::itn::normalize;

fn bench_normalize_short(c: &mut Criterion) {
    let inputs = vec![
        "hello world",
        "it is twenty five degrees outside",
        "the meeting is at three thirty pm",
        "i need one hundred dollars",
        "call me at four oh five five five one two three four",
    ];
    c.bench_function("itn_normalize_short", |b| {
        b.iter(|| {
            for s in &inputs {
                let _ = normalize(black_box(s));
            }
        });
    });
}

fn bench_normalize_medium(c: &mut Criterion) {
    let text = "on january twenty third two thousand twenty four at seven forty five pm the total was one thousand two hundred fifty three dollars and seventy five cents";
    c.bench_function("itn_normalize_medium", |b| {
        b.iter(|| {
            let _ = normalize(black_box(text));
        });
    });
}

criterion_group!(benches, bench_normalize_short, bench_normalize_medium);
criterion_main!(benches);
