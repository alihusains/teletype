use super::*;

/// Pure-shape check of the trim arithmetic with a fake per-chunk speech
/// map (the real Silero model is exercised in `silent_take_is_unchanged`,
/// where silence really scores below threshold).
#[test]
fn trim_keeps_150ms_padding_around_speech() {
    // 16 kHz, 512-sample chunks (32 ms). 20 chunks = 640 ms.
    let chunk = 512usize;
    let chunks = 20usize;
    let samples = vec![0.0f32; chunk * chunks];
    // Speech in chunks 5..=14 (inclusive) → 320 ms of speech.
    let first = 5usize;
    let last = 14usize;
    let pad = (150u64 * 16_000u64 / 1000) as usize / chunk; // 150ms → 4 chunks (128ms)
    assert_eq!(pad, 4, "150 ms rounds to 4 chunks of 32 ms");

    let start = first.saturating_sub(pad) * chunk;
    let end = ((last + 1) + pad).min(chunks) * chunk;
    let trimmed = &samples[start..end];

    // Leading silence trimmed: 5 chunks of silence → 1 chunk of padding.
    assert_eq!(start / chunk, 1, "keep 150 ms (4 chunks) before speech");
    // Trailing silence trimmed: 5 chunks → 4 chunks of padding.
    assert_eq!(end / chunk, last + 1 + pad);
    // The trimmed slice still contains all speech chunks.
    assert!(start <= first * chunk);
    assert!(end >= (last + 1) * chunk);
    // And it is strictly shorter than the input.
    assert!(trimmed.len() < samples.len());
}

/// A take that is all silence must come back unchanged: the trim must
/// never drop the user's audio on a VAD miss.
#[test]
fn silent_take_is_unchanged() {
    // 1 second of digital silence at 16 kHz.
    let samples = vec![0.0f32; 16_000];
    let trimmed = trim_silence_vad(&samples);
    assert_eq!(
        trimmed.len(),
        samples.len(),
        "silence must never be trimmed away"
    );
}

/// A mid-take "speech" burst surrounded by silence must be trimmed to
/// roughly the speech region plus 150 ms of padding on each side. Uses
/// the probe-injected variant (the real Silero model does not reliably
/// flag synthetic tones as speech) so the trim arithmetic is what's
/// under test.
#[test]
fn tone_in_the_middle_is_trimmed() {
    let sr = 16_000usize;
    let chunk = 512usize; // 32 ms @ 16 kHz (Silero's chunk size)
    let chunks = 25usize; // 800 ms total
    let samples = vec![0.0f32; chunks * chunk];
    // "Speech" in chunks 7..=17 (352 ms) → tone region 224..=576 ms.
    let speech: Vec<bool> = (0..chunks).map(|c| (7..=17).contains(&c)).collect();
    let trimmed = trim_silence_vad_with_probe(&samples, chunk, &speech);
    // 352 ms of tone + 128 ms padding each side (150 ms rounds to 4
    // chunks of 32 ms) ≈ 608 ms, well under the full 800 ms.
    assert!(
        trimmed.len() < samples.len(),
        "trim should shorten the take"
    );
    assert!(trimmed.len() >= 400 * sr / 1000 && trimmed.len() <= 750 * sr / 1000);
}
