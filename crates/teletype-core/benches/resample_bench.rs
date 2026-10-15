//! Resample bench: 10 s of 48 kHz mono audio converted to 16 kHz.
//!
//! Exercises `audio::resample_to_target` (currently linear interpolation),
//! the function P5.3 proposes to replace with rubato. This bench is the
//! "before" number for that change.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use teletype_core::audio::resample_to_target;

fn ten_seconds_48k() -> Vec<f32> {
    // 440 Hz tone at 48 kHz, deterministic (no RNG in the hot path).
    (0..480_000)
        .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 48_000.0).sin() as f32 * 0.5)
        .collect()
}

fn resample_48k_to_16k(c: &mut Criterion) {
    let pcm = ten_seconds_48k();
    assert_eq!(pcm.len(), 480_000);
    c.bench_function("resample_10s_48k_to_16k", |b| {
        b.iter(|| black_box(resample_to_target(black_box(&pcm), black_box(48_000))))
    });
}

criterion_group!(benches, resample_48k_to_16k);
criterion_main!(benches);
