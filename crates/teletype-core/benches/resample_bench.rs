use criterion::{black_box, criterion_group, criterion_main, Criterion};
use teletype_core::audio::resample_to_target;

fn bench_resample(c: &mut Criterion) {
    // 48kHz -> 16kHz (3:1)
    let input: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.01).sin()).collect();
    c.bench_function("resample_48k_to_16k", |b| {
        b.iter(|| {
            let _ = resample_to_target(black_box(&input), 16000);
        });
    });
}

criterion_group!(benches, bench_resample);
criterion_main!(benches);
