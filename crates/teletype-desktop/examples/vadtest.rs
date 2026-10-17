fn main() {
    let sr = 16_000usize;
    let chunks = 31usize;
    let chunk = 512usize;
    let mut samples = vec![0.0f32; chunks * chunk];
    // Sawtooth (rich harmonics, more speech-like) in chunks 9..=21
    for c in 9..=21 {
        for i in 0..chunk {
            samples[c * chunk + i] =
                (2.0 * ((c * chunk + i) as f64 / (sr as f64 * 0.5)) - 1.0) as f32 * 0.5;
        }
    }
    let mut engine = silero_vad_pure::SileroVad::new(silero_vad_pure::SampleRate::Hz16000).unwrap();
    for c in 0..chunks {
        let p = engine
            .process(&samples[c * chunk..(c + 1) * chunk])
            .unwrap();
        if (0..3).contains(&c) || (8..=22).contains(&c) || (chunks - 3..).contains(&c) {
            eprintln!("chunk {c}: p={p:.3}");
        }
    }
}
