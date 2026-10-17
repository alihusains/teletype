//! Noise-handling modes (plan §5.5) and the pre-speech SNR auto-enable rule
//! for Aggressive mode.
//!
//! The policy (plan §5.0): the default does NOT denoise the ASR input.
//! DFN3 feeds the VAD/endpoint/level path only. Aggressive (denoise the ASR
//! input too) is experimental: it is auto-enabled for a take only when the
//! measured pre-speech SNR is below 0 dB, and only once the P5.8 eval proves
//! it helps.
//!
//! The wire value is the existing persisted setting
//! `noise_reduction_enabled` (bool, default true), which the UI renders as
//! the Off / Smart / Aggressive selector. `true` = Smart (the default),
//! `false` = Off. Aggressive is a runtime decision, not a persisted value:
//! [`aggressive_auto_enabled`] decides per take from the pre-speech SNR.

/// The three Noise handling modes (plan §5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseHandlingMode {
    /// No denoising anywhere: raw audio to ASR and to VAD/level.
    Off,
    /// Denoise for DETECTION only (VAD, endpointing, level meter). The ASR
    /// input stays raw (plan §5.0 policy). This is the default.
    Smart,
    /// Also denoises the ASR input. Experimental: auto-enabled per take only
    /// when the pre-speech SNR is below 0 dB (and the P5.8 eval approves).
    Aggressive,
}

impl NoiseHandlingMode {
    /// Maps the persisted `noise_reduction_enabled` bool to a mode.
    ///
    /// `true` = Smart (the default, denoise detection), `false` = Off.
    /// Aggressive is never persisted; it is a per-take runtime decision
    /// (see [`aggressive_auto_enabled`]).
    pub fn from_setting(noise_reduction_enabled: bool) -> Self {
        if noise_reduction_enabled {
            Self::Smart
        } else {
            Self::Off
        }
    }

    /// Whether this mode denoises the VAD/endpoint/level path.
    pub fn denoises_detection(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether this mode denoises the ASR input.
    pub fn denoises_asr(self) -> bool {
        matches!(self, Self::Aggressive)
    }
}

/// The pre-speech SNR below which Aggressive is auto-enabled for a take
/// (plan §5.5). At or below 0 dB the noise is as loud as the speech on
/// average, which is where full denoising can only help.
pub const AGGRESSIVE_SNR_THRESHOLD_DB: f32 = 0.0;

/// Decides whether Aggressive is auto-enabled for a take from the measured
/// pre-speech SNR (plan §5.5).
///
/// `pre_speech_snr_db` is the SNR of the audio BEFORE the first speech
/// frame of the take (the pre-roll + leading silence). `None` (no speech
/// seen, or no noise measured) means there is no evidence either way:
/// Aggressive stays off. The P5.8 eval gate is a separate flag the caller
/// owns (the eval harness flips it; production keeps it off until the
/// eval proves Aggressive beats raw at 0 dB).
pub fn aggressive_auto_enabled(pre_speech_snr_db: Option<f32>, eval_approved: bool) -> bool {
    let Some(snr) = pre_speech_snr_db else {
        return false;
    };
    snr < AGGRESSIVE_SNR_THRESHOLD_DB && eval_approved
}

/// Measures the pre-speech SNR of a take in dB.
///
/// `samples` is the take's mono audio in [-1, 1] at `rate` Hz;
/// `first_speech_sample` is the index of the first speech sample (from the
/// VAD flags), or `None` when no speech was seen. The "pre-speech" region is
/// everything before the first speech sample; the "speech" region is the
/// first [`SPEECH_WINDOW_SECS`] seconds after it (a short window keeps the
/// estimate from being dominated by a long utterance).
///
/// Returns `None` when there is no speech, or the pre-speech region is too
/// short to measure (below [`MIN_PRE_SPEECH_SECS`] seconds).
pub fn pre_speech_snr(
    samples: &[f32],
    rate: u32,
    first_speech_sample: Option<usize>,
) -> Option<f32> {
    let rate = rate.max(1) as usize;
    let first = first_speech_sample?;
    if first >= samples.len() {
        return None;
    }
    let min_pre = (MIN_PRE_SPEECH_SECS as usize) * rate;
    if first < min_pre {
        return None;
    }
    let pre = &samples[..first];
    let speech_end = (first + SPEECH_WINDOW_SECS as usize * rate).min(samples.len());
    let speech = &samples[first..speech_end];
    let pre_rms = rms(pre);
    let speech_rms = rms(speech);
    if pre_rms <= 0.0 || speech_rms <= 0.0 {
        return None;
    }
    Some(20.0f32 * (speech_rms / pre_rms).log10())
}

/// Minimum pre-speech audio (seconds) before an SNR estimate is meaningful.
const MIN_PRE_SPEECH_SECS: u32 = 1;
/// Speech window (seconds) used for the SNR's speech side.
const SPEECH_WINDOW_SECS: u32 = 3;

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    let sum: f64 = x.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / x.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, rate: u32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * freq as f64 * i as f64 / rate as f64).sin() as f32
                    * amp
            })
            .collect()
    }

    #[test]
    fn setting_true_is_smart_false_is_off() {
        assert_eq!(
            NoiseHandlingMode::from_setting(true),
            NoiseHandlingMode::Smart
        );
        assert_eq!(
            NoiseHandlingMode::from_setting(false),
            NoiseHandlingMode::Off
        );
    }

    #[test]
    fn smart_denoises_detection_only() {
        assert!(NoiseHandlingMode::Smart.denoises_detection());
        assert!(!NoiseHandlingMode::Smart.denoises_asr());
        assert!(!NoiseHandlingMode::Off.denoises_detection());
        assert!(NoiseHandlingMode::Aggressive.denoises_asr());
    }

    #[test]
    fn aggressive_needs_negative_snr_and_eval() {
        assert!(!aggressive_auto_enabled(Some(-5.0), false), "eval gate off");
        assert!(aggressive_auto_enabled(Some(-5.0), true), "SNR < 0 + eval");
        assert!(!aggressive_auto_enabled(Some(5.0), true), "SNR > 0");
        assert!(!aggressive_auto_enabled(None, true), "no measurement");
    }

    #[test]
    fn pre_speech_snr_positive_when_speech_louder() {
        let rate = 16_000u32;
        // 2 s of quiet noise (0.01 amp), then 3 s of loud speech (0.5 amp).
        let mut samples = tone(100.0, rate, 2 * rate as usize, 0.01);
        samples.extend(tone(440.0, rate, 3 * rate as usize, 0.5));
        let snr = pre_speech_snr(&samples, rate, Some(2 * rate as usize)).expect("measurable");
        // 20*log10(0.5/0.01) = 34 dB (RMS ratio ~ same for sines).
        assert!((snr - 34.0).abs() < 1.0, "SNR ~ 34 dB, got {snr}");
    }

    #[test]
    fn pre_speech_snr_negative_when_noise_louder() {
        let rate = 16_000u32;
        let mut samples = tone(100.0, rate, 2 * rate as usize, 0.5);
        samples.extend(tone(440.0, rate, 3 * rate as usize, 0.01));
        let snr = pre_speech_snr(&samples, rate, Some(2 * rate as usize)).expect("measurable");
        assert!(snr < 0.0, "noise louder than speech: SNR < 0, got {snr}");
    }

    #[test]
    fn pre_speech_snr_none_without_speech_or_with_short_preamble() {
        let rate = 16_000u32;
        let samples = tone(100.0, rate, 4 * rate as usize, 0.1);
        assert_eq!(pre_speech_snr(&samples, rate, None), None);
        // Speech at 0.2 s: less than MIN_PRE_SPEECH_SECS of pre-speech.
        assert_eq!(
            pre_speech_snr(&samples, rate, Some((0.2 * rate as f32) as usize)),
            None
        );
    }
}
