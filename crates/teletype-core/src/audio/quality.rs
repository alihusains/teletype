//! Per-take audio quality signals (plan §P5.7, data layer only).
//!
//! [`take_quality`] computes deterministic, side-effect-free quality
//! metrics from the raw take samples plus the per-frame VAD flags that the
//! dictation pipeline already produces. The result is a [`TakeQuality`]
//! that the dictation-wiring unit (a later change) attaches to the take's
//! [`crate::trace::DictationTrace`] fields (`speech_rms_db`,
//! `noise_rms_db`, `snr_db`, `clipped`, `bt_hfp`).
//!
//! Nothing in this module reads the microphone, holds state, or touches
//! the filesystem: it is pure arithmetic on the caller's slices, so it is
//! trivially unit-testable with synthetic signals.

/// Frame size for the per-frame VAD flags the pipeline feeds us: 32 ms at
/// 16 kHz (the Silero VAD chunk rate, see [`crate::vad`]).
const FRAME_SIZE: usize = 512;

/// A sample at or above this magnitude counts as "clipping": the take is
/// one step away from digital overload. 0.999 leaves a 0.1 % margin below
/// the full-scale limit of 1.0 so a legitimate loud peak just under full
/// scale does not get flagged.
const CLIP_THRESHOLD: f32 = 0.999;

/// A take is "clipped" when at least this fraction of its samples reach
/// [`CLIP_THRESHOLD`. A single hot sample is a transient, not clipping.
const CLIP_FRACTION: f32 = 0.001;

/// A frame is "silent" (excluded from the noise estimate) when its RMS is
/// at or below this level. In dBFS this is -90 dB, well below any real
/// microphone noise floor, so a truly silent frame is just digital silence
/// and must not drag the noise estimate down.
const SILENT_FRAME_RMS: f32 = 1e-4;

/// Per-take audio quality signals (plan §P5.7). All fields are `Option`
/// because a take can be too short, too quiet, or otherwise lacking the
/// signal a given metric needs; the trace line omits `None` fields
/// (`skip_serializing_if`) so old trace files stay valid.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TakeQuality {
    /// RMS of the speech frames (VAD-true) in dBFS, or `None` when the take
    /// has no speech frame.
    pub speech_rms_db: Option<f32>,
    /// RMS of the noise frames (VAD-false, non-silent) in dBFS, or `None`
    /// when the take has no such frame (all speech, or all digital silence).
    pub noise_rms_db: Option<f32>,
    /// `speech_rms_db - noise_rms_db`, the signal-to-noise ratio in dB, or
    /// `None` when either side is missing.
    pub snr_db: Option<f32>,
    /// Whether at least [`CLIP_FRACTION`] of the samples reached
    /// [`CLIP_THRESHOLD`]. `None` for an empty take.
    pub clipped: Option<bool>,
    /// The take's Bluetooth-HFP flag, passed through unchanged from the
    /// caller (the caller knows the active input device; this module does
    /// not). `None` when the caller could not determine it.
    pub bt_hfp: Option<bool>,
}

/// Compute per-take quality signals from raw samples and per-frame VAD
/// flags.
///
/// `samples` is the take's full audio in [-1, 1]; `vad_flags` is one bool
/// per [`FRAME_SIZE`]-sample frame in the same order (a shorter tail of
/// `samples` beyond `vad_flags.len() * FRAME_SIZE` is still counted for
/// clipping but belongs to no frame). `bt_hfp` is passed through to the
/// result unchanged.
///
/// Deterministic: the same inputs always produce the same output, and no
/// input mutates the result.
pub fn take_quality(samples: &[f32], vad_flags: &[bool], bt_hfp: Option<bool>) -> TakeQuality {
    // Clipping is a property of the whole take, independent of the VAD.
    let clipped = if samples.is_empty() {
        None
    } else {
        let hot = samples.iter().filter(|s| s.abs() >= CLIP_THRESHOLD).count();
        Some(hot as f32 / samples.len() as f32 >= CLIP_FRACTION)
    };

    // Per-frame RMS, split by the VAD flag.
    let mut speech_sq: f64 = 0.0;
    let mut speech_n: usize = 0;
    let mut noise_sq: f64 = 0.0;
    let mut noise_n: usize = 0;
    for (i, &is_speech) in vad_flags.iter().enumerate() {
        let frame = &samples[i * FRAME_SIZE..(i + 1) * FRAME_SIZE.min(samples.len())];
        if frame.is_empty() {
            continue;
        }
        let rms = frame.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / frame.len() as f64;
        let rms = rms.sqrt();
        if is_speech {
            speech_sq += rms * rms;
            speech_n += 1;
        } else if rms > SILENT_FRAME_RMS as f64 {
            // VAD-false but not digital silence: this is the noise floor.
            noise_sq += rms * rms;
            noise_n += 1;
        }
    }

    let speech_rms_db = if speech_n > 0 {
        Some(rms_to_dbfs((speech_sq / speech_n as f64).sqrt()))
    } else {
        None
    };
    let noise_rms_db = if noise_n > 0 {
        Some(rms_to_dbfs((noise_sq / noise_n as f64).sqrt()))
    } else {
        None
    };
    let snr_db = match (speech_rms_db, noise_rms_db) {
        (Some(s), Some(n)) => Some(s - n),
        _ => None,
    };

    TakeQuality {
        speech_rms_db,
        noise_rms_db,
        snr_db,
        clipped,
        bt_hfp,
    }
}

/// RMS (as a linear amplitude) to dBFS. 0 maps to the -120 dB floor so a
/// silent-but-nonzero frame never produces -inf.
fn rms_to_dbfs(rms: f64) -> f32 {
    let db = 20.0 * rms.log10();
    (db.max(-120.0)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine tone at `freq` Hz, `rate` sample rate, `samples` long, peak
    /// `amp`. Deterministic; no RNG.
    fn tone(freq: f32, rate: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect()
    }

    fn silence(n: usize) -> Vec<f32> {
        vec![0.0; n]
    }

    /// All-true VAD flags for `n` frames of `FRAME_SIZE` samples.
    fn all_speech(n_frames: usize) -> Vec<bool> {
        vec![true; n_frames]
    }

    /// All-false VAD flags for `n` frames of `FRAME_SIZE` samples.
    fn all_noise(n_frames: usize) -> Vec<bool> {
        vec![false; n_frames]
    }

    #[test]
    fn empty_take_is_all_none_except_no_clip() {
        let q = take_quality(&[], &[], None);
        assert!(q.speech_rms_db.is_none());
        assert!(q.noise_rms_db.is_none());
        assert!(q.snr_db.is_none());
        // No samples: we cannot say it clipped or not.
        assert!(q.clipped.is_none());
        assert_eq!(q.bt_hfp, None);
    }

    #[test]
    fn pure_digital_silence_has_no_speech_or_noise() {
        // 1 second of digital silence, all VAD-false: there is no speech
        // and no real noise (the frames are exactly 0, below the silent
        // threshold), so both RMS fields are None. Clipping is Some(false).
        let samples = silence(16_000);
        let flags = all_noise(16_000 / FRAME_SIZE);
        let q = take_quality(&samples, &flags, Some(false));
        assert!(q.speech_rms_db.is_none());
        assert!(q.noise_rms_db.is_none());
        assert!(q.snr_db.is_none());
        assert_eq!(q.clipped, Some(false));
        assert_eq!(q.bt_hfp, Some(false));
    }

    #[test]
    fn loud_tone_reports_speech_rms_near_full_scale() {
        // A 0.5-amplitude sine has RMS amp/sqrt(2) ~ 0.3535, which is
        // 20*log10(0.3535) ~ -9 dBFS. VAD-true: it is the speech estimate.
        let samples = tone(440.0, 16_000.0, 16_000, 0.5);
        let flags = all_speech(16_000 / FRAME_SIZE);
        let q = take_quality(&samples, &flags, None);
        let speech = q.speech_rms_db.expect("a loud tone is speech");
        assert!(
            (speech - (-9.0)).abs() < 0.5,
            "speech RMS {speech} dBFS should be near -9 dBFS"
        );
        assert!(q.noise_rms_db.is_none(), "no VAD-false frames: no noise");
        assert!(q.snr_db.is_none(), "no noise side: no SNR");
        assert_eq!(q.clipped, Some(false), "0.5 amp does not clip");
    }

    #[test]
    fn clipped_tone_flags_clipped() {
        // A 1.0-amplitude sine spends most of its cycle at or above 0.999
        // near the peaks; the fraction is small but well over 0.1 %.
        let samples = tone(440.0, 16_000.0, 16_000, 1.0);
        let flags = all_speech(16_000 / FRAME_SIZE);
        let q = take_quality(&samples, &flags, None);
        assert_eq!(q.clipped, Some(true), "full-scale sine must clip");
    }

    #[test]
    fn single_hot_sample_does_not_clip() {
        // One hot sample in a sea of quiet: below the 0.1 % fraction, so
        // the take is not clipped. Guards against a transient being read
        // as overload.
        let mut samples = silence(16_000);
        samples[100] = 1.0;
        let flags = all_noise(16_000 / FRAME_SIZE);
        let q = take_quality(&samples, &flags, None);
        assert_eq!(q.clipped, Some(false), "one hot sample is not clipping");
    }

    #[test]
    fn speech_plus_noise_mix_reports_positive_snr() {
        // First half: a 0.5-amp tone, VAD-true (speech). Second half: a
        // 0.05-amp tone, VAD-false (noise floor). The SNR comes out positive
        // and equals the difference of the two per-frame RMS estimates.
        let half = 8_000;
        let mut samples = tone(440.0, 16_000.0, half, 0.5);
        samples.extend(tone(1000.0, 16_000.0, half, 0.05));
        let mut flags = all_speech(half / FRAME_SIZE);
        flags.extend(all_noise(half / FRAME_SIZE));
        let q = take_quality(&samples, &flags, Some(true));
        let speech = q.speech_rms_db.expect("speech side present");
        let noise = q.noise_rms_db.expect("noise side present");
        let snr = q.snr_db.expect("both sides present: SNR defined");
        assert!(
            (speech - (-9.0)).abs() < 0.5,
            "speech ~ -9 dBFS, got {speech}"
        );
        assert!(
            (noise - (-22.0)).abs() < 0.5,
            "noise ~ -22 dBFS, got {noise}"
        );
        assert!(
            (snr - (speech - noise)).abs() < 1e-3,
            "SNR = speech - noise"
        );
        assert!(snr > 0.0, "SNR must be positive, got {snr}");
        assert_eq!(q.bt_hfp, Some(true), "bt_hfp passes through");
        assert_eq!(q.clipped, Some(false));
    }

    #[test]
    fn short_tail_beyond_flags_still_counts_for_clipping() {
        // `samples` is longer than `vad_flags` covers: the tail belongs to
        // no frame (no speech/noise contribution) but still counts for
        // clipping. Make the tail all full-scale so clipping trips.
        let framed = FRAME_SIZE; // one frame of silence, VAD-false
        let mut samples = silence(framed);
        samples.extend(std::iter::repeat_n(1.0, FRAME_SIZE)); // hot tail
        let flags = all_noise(1);
        let q = take_quality(&samples, &flags, None);
        assert_eq!(
            q.clipped,
            Some(true),
            "a full-scale tail must still flag clipping"
        );
        // The framed half is digital silence: no noise estimate.
        assert!(q.noise_rms_db.is_none());
    }
}
