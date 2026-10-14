//! Criterion benchmarks for the audio resampler.
//!
//! P7 in the roadmap: the current implementation uses linear interpolation
//! with no anti-aliasing filter. These benches establish a baseline so we
//! can measure the before/after when we replace it with a proper FIR/sinc
//! resampler (e.g. `rubato`).
//!
//! Run with: `cargo bench -p teletype-core --bench audio`

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use teletype_core::audio::resample_to_target;

/// Generate a synthetic 1-second mono signal at `sample_rate` Hz.
/// Uses a sine wave so the signal is not all zeros (which would make the
/// resampler a trivial constant-propagation target in a hot compiler).
fn sine_signal(sample_rate: u32, freq_hz: f64) -> Vec<f32> {
    let n = sample_rate as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / sample_rate as f64;
            (2.0 * std::f64::consts::PI * freq_hz * t).sin() as f32
        })
        .collect()
}

fn bench_resample_48k_to_16k(c: &mut Criterion) {
    // Most common case: standard Bluetooth/USB microphone at 48 kHz → 16 kHz.
    // 1 second of audio = 48,000 samples input, 16,000 output.
    let signal = sine_signal(48_000, 440.0);
    c.bench_function("audio/resample_48k_to_16k_1s", |b| {
        b.iter(|| {
            criterion::black_box(resample_to_target(
                criterion::black_box(&signal),
                48_000,
            ))
        })
    });
}

fn bench_resample_44k_to_16k(c: &mut Criterion) {
    // CD-quality input — less common but supported.
    let signal = sine_signal(44_100, 440.0);
    c.bench_function("audio/resample_44k_to_16k_1s", |b| {
        b.iter(|| {
            criterion::black_box(resample_to_target(
                criterion::black_box(&signal),
                44_100,
            ))
        })
    });
}

fn bench_resample_various_durations(c: &mut Criterion) {
    // Typical dictations range from a word (≈0.5 s) to a paragraph (≈20 s).
    // Bench several durations at 48 kHz to verify linear scaling.
    let mut group = c.benchmark_group("audio/resample_48k_to_16k");
    for seconds in [0u32, 1, 5, 10, 20] {
        let signal: Vec<f32> = if seconds == 0 {
            Vec::new()
        } else {
            sine_signal(48_000 * seconds, 440.0)
        };
        group.bench_with_input(
            BenchmarkId::new("seconds", seconds),
            &signal,
            |b, s| {
                b.iter(|| {
                    criterion::black_box(resample_to_target(
                        criterion::black_box(s),
                        48_000,
                    ))
                })
            },
        );
    }
    group.finish();
}

fn bench_resample_identity(c: &mut Criterion) {
    // When the input is already 16 kHz, the function returns a clone.
    // This is the fast path; it should be near-free.
    let signal = sine_signal(16_000, 440.0);
    c.bench_function("audio/resample_identity_16k", |b| {
        b.iter(|| {
            criterion::black_box(resample_to_target(
                criterion::black_box(&signal),
                16_000,
            ))
        })
    });
}

criterion_group!(
    benches,
    bench_resample_48k_to_16k,
    bench_resample_44k_to_16k,
    bench_resample_various_durations,
    bench_resample_identity
);
criterion_main!(benches);
