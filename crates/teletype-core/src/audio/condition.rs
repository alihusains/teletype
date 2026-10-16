//! Light capture conditioning for the 16 kHz mono mic path (plan §5.4).
//!
//! Pure Rust, no new dependencies. The chain is intentionally small — the
//! plan keeps it "light" and behind an eval judgment — and every stage is
//! stateful only in the way a filter must be:
//!
//! 1. DC removal: one-pole high-pass, fc ≈ 20 Hz.
//! 2. Rumble high-pass: 2nd-order 80 Hz Butterworth biquad (transposed
//!    direct-form II; coefficients derived once in [`new`]).
//! 3. Slow AGC: targets −20 dBFS RMS speech, max +18 dB gain, 50 ms attack /
//!    500 ms release at 16 kHz.
//! 4. Peak limiter at −1 dBFS.
//! 5. Clipping detection: fraction of samples with |s| ≥ 0.99; > 0.1% marks
//!    the take `clipped`.
//!
//! Shipping is WIRED-BUT-OFF: [`AUDIO_CONDITIONING_DEFAULT`] is `false`. The
//! Phase 5 Gate enable is an eval decision (data-pending), not a code change.

/// Whether light conditioning is enabled by default. Wired but off: the
/// Phase 5 Gate decides on eval data (plan §5.4).
pub const AUDIO_CONDITIONING_DEFAULT: bool = false;

/// Conditioning constants, documented per plan §5.4.
#[derive(Debug, Clone, Copy)]
pub struct Consts {
    /// DC-removal one-pole high-pass corner frequency (Hz).
    pub dc_fc_hz: f64,
    /// Rumble high-pass corner frequency (Hz), 2nd-order Butterworth.
    pub hp_fc_hz: f64,
    /// Sample rate the coefficients are derived for (Hz).
    pub sample_rate: f64,
    /// AGC target speech level (dBFS RMS).
    pub agc_target_db: f32,
    /// AGC maximum gain (dB).
    pub agc_max_gain_db: f32,
    /// AGC attack time (seconds) — how fast gain follows rising level.
    pub agc_attack_s: f64,
    /// AGC release time (seconds) — how fast gain follows falling level.
    pub agc_release_s: f64,
    /// Limiter ceiling (amplitude), i.e. −1 dBFS.
    pub limiter_ceiling: f32,
    /// Clipping threshold (amplitude): samples with |s| ≥ this count.
    pub clip_threshold: f32,
    /// Fraction of samples above [`clip_threshold`](Self::clip_threshold)
    /// that marks the take as clipped.
    pub clip_pct: f32,
}

impl Default for Consts {
    fn default() -> Self {
        Self {
            dc_fc_hz: 20.0,
            hp_fc_hz: 80.0,
            sample_rate: 16_000.0,
            agc_target_db: -20.0,
            agc_max_gain_db: 18.0,
            agc_attack_s: 0.05,
            agc_release_s: 0.5,
            limiter_ceiling: (10f32.powf(-1.0 / 20.0)), // −1 dBFS ≈ 0.891
            clip_threshold: 0.99,
            clip_pct: 0.001,
        }
    }
}

/// One pass's conditioning report.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConditionStats {
    /// True when more than [`Consts::clip_pct`] of samples hit the ceiling.
    pub clipped: bool,
    /// Fraction (0..1) of samples with |s| ≥ the clip threshold.
    pub pct_clipped: f32,
    /// The AGC gain actually applied this pass (dB).
    pub applied_gain_db: f32,
}

/// A stateful light conditioner for 16 kHz mono f32 (plan §5.4).
///
/// Built with [`new`](Self::new) (or [`with_consts`](Self::with_consts));
/// feed it with [`process`](Self::process). All state is internal; the same
/// instance can process a whole take.
#[derive(Debug)]
pub struct Conditioner {
    c: Consts,
    // DC removal (one-pole HP): y[n] = a*(y[n-1] + x[n] - x[n-1])
    dc_prev_x: f32,
    dc_prev_y: f32,
    dc_a: f32,
    // 80 Hz Butterworth HP, transposed direct-form II
    hp_b0: f32,
    hp_b1: f32,
    hp_b2: f32,
    hp_a1: f32,
    hp_a2: f32,
    hp_w1: f32,
    hp_w2: f32,
    // AGC
    agc_gain: f32,     // current gain (linear)
    agc_max_gain: f32, // linear
    agc_target: f32,   // linear RMS
    agc_attack: f32,   // per-sample coefficient
    agc_release: f32,  // per-sample coefficient
    // scratch for the pass
    scratch: Vec<f32>,
}

impl Conditioner {
    /// New conditioner with the documented plan §5.4 constants.
    pub fn new() -> Self {
        Self::with_consts(Consts::default())
    }

    /// New conditioner with explicit constants (tests, tuning).
    pub fn with_consts(c: Consts) -> Self {
        let (dc_a,) = Self::dc_coeffs(c.dc_fc_hz, c.sample_rate);
        let (b0, b1, b2, a1, a2) = Self::butterworth_hp_coeffs(c.hp_fc_hz, c.sample_rate);
        let agc_max_gain = 10f32.powf(c.agc_max_gain_db / 20.0);
        let agc_target = 10f32.powf(c.agc_target_db / 20.0);
        // Convert attack/release times to per-sample linear-smoothing
        // coefficients: step = 1 / (time * sample_rate) samples.
        let agc_attack = (1.0 / (c.agc_attack_s * c.sample_rate)) as f32;
        let agc_release = (1.0 / (c.agc_release_s * c.sample_rate)) as f32;
        Self {
            c,
            dc_prev_x: 0.0,
            dc_prev_y: 0.0,
            dc_a,
            hp_b0: b0,
            hp_b1: b1,
            hp_b2: b2,
            hp_a1: a1,
            hp_a2: a2,
            hp_w1: 0.0,
            hp_w2: 0.0,
            agc_gain: 1.0,
            agc_max_gain,
            agc_target,
            agc_attack,
            agc_release,
            scratch: Vec::new(),
        }
    }

    /// One-pole high-pass (DC removal) coefficient `a` for corner `fc`.
    ///
    /// Discretized as `a = (1 - c1) / (1 + c1)` with `c1 = 2π fc / fs`
    /// (the standard first-order DC block; gain ≈ 0 dB above ~2·fc and
    /// −3 dB at fc).
    fn dc_coeffs(fc: f64, fs: f64) -> (f32,) {
        let c1 = 2.0 * std::f64::consts::PI * fc / fs;
        let a = (1.0 - c1) / (1.0 + c1);
        (a as f32,)
    }

    /// 2nd-order Butterworth high-pass coefficients (b0,b1,b2,a1,a2),
    /// normalized so a0 = 1. Derived from the standard analog prototype
    /// H(s) = s^2 / (s^2 + sqrt(2) s + 1) at fc, via bilinear transform.
    fn butterworth_hp_coeffs(fc: f64, fs: f64) -> (f32, f32, f32, f32, f32) {
        let w0 = 2.0 * std::f64::consts::PI * fc / fs;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / 2.0 * 2.0f64.sqrt(); // Q = 1/sqrt(2) for Butterworth
        let a0 = 1.0 + alpha;
        let b0 = (1.0 + cw) / (2.0 * a0);
        let b1 = -(1.0 + cw) / a0;
        let b2 = (1.0 + cw) / (2.0 * a0);
        let a1 = -2.0 * cw / a0;
        let a2 = (1.0 - alpha) / a0;
        (b0 as f32, b1 as f32, b2 as f32, a1 as f32, a2 as f32)
    }

    /// Runs the full chain over `samples` (in place) and returns stats.
    ///
    /// `samples` is 16 kHz mono f32. The function mutates `samples` to the
    /// conditioned audio and reports clipping + applied gain.
    pub fn process(&mut self, samples: &mut [f32]) -> ConditionStats {
        if samples.is_empty() {
            return ConditionStats {
                clipped: false,
                pct_clipped: 0.0,
                applied_gain_db: 0.0,
            };
        }

        self.scratch.clear();
        self.scratch.extend_from_slice(samples);
        let n = samples.len();

        // Stage 1: DC removal (one-pole HP).
        for i in 0..n {
            let x = self.scratch[i];
            let y = self.dc_a * (self.dc_prev_y + x - self.dc_prev_x);
            self.dc_prev_x = x;
            self.dc_prev_y = y;
            self.scratch[i] = y;
        }

        // Stage 2: 80 Hz Butterworth HP (transposed direct-form II).
        for i in 0..n {
            let x = self.scratch[i];
            let y = self.hp_b0 * x + self.hp_w1;
            self.hp_w1 = self.hp_b1 * x - self.hp_a1 * y + self.hp_w2;
            self.hp_w2 = self.hp_b2 * x - self.hp_a2 * y;
            self.scratch[i] = y;
        }

        // Stage 3: slow AGC.
        // Measure the RMS of the whole pass as the speech-level proxy, then
        // move the gain toward the target with attack/release coefficients.
        // (Per the plan's "slow AGC", one update per `process` call; the
        // desktop wiring feeds it per-frame.)
        let rms = (self
            .scratch
            .iter()
            .map(|s| f64::from(*s) * f64::from(*s))
            .sum::<f64>()
            / n as f64)
            .sqrt() as f32;
        let target_gain = if rms > 1e-7 {
            // Never attenuate (min gain 0 dB); cap at the max gain.
            (self.agc_target / rms).min(self.agc_max_gain).max(1.0)
        } else {
            // Silence: hold current gain (don't pump up noise).
            self.agc_gain
        };
        // Move gain toward target: fast on the way down (attack when level
        // rises -> gain should fall), slow on the way up (release).
        // Per-sample step = 1 / (time * sample_rate); over a frame of `n`
        // samples the effective step is n times that (capped at 1.0 so a
        // very long frame converges in one step instead of overshooting).
        let per_sample = if target_gain < self.agc_gain {
            self.agc_attack
        } else {
            self.agc_release
        };
        let step = (per_sample * n as f32).min(1.0);
        let mut next = self.agc_gain + (target_gain - self.agc_gain) * step;
        // Never attenuate: the AGC only adds gain, capped at the max.
        next = next.clamp(1.0, self.agc_max_gain);
        self.agc_gain = next;
        let applied = self.agc_gain;

        // Stage 4: apply gain, detect clipping on the pre-limiter signal,
        // then limit. Clipping is detected BEFORE the limiter so the limiter
        // does not hide the source of the clipping (the stat reports that the
        // INPUT was hot, not that the limiter clamped).
        let mut clipped_count = 0usize;
        for (i, g) in self.scratch.iter().enumerate().take(n) {
            let g = *g * applied;
            if g.abs() >= self.c.clip_threshold {
                clipped_count += 1;
            }
            // Limiter: hard clamp to the ceiling (−1 dBFS).
            let s = g.clamp(-self.c.limiter_ceiling, self.c.limiter_ceiling);
            samples[i] = s;
        }

        let pct_clipped = clipped_count as f32 / n as f32;
        let clipped = pct_clipped > self.c.clip_pct;
        let applied_gain_db = 20.0 * applied.log10();

        ConditionStats {
            clipped,
            pct_clipped,
            applied_gain_db,
        }
    }
}

impl Default for Conditioner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq_hz: f64, amp: f32, n: usize, rate: f64) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * freq_hz * i as f64 / rate).sin() as f32)
            .collect()
    }

    #[test]
    fn dc_offset_is_removed() {
        // A DC offset of +0.3 (pure constant) must be removed: the output
        // mean should be near zero, not 0.3.
        let mut c = Conditioner::new();
        let n = 16_000; // 1 s
        let mut s: Vec<f32> = vec![0.3; n];
        // add a tiny bit of signal so AGC doesn't see pure silence
        for i in 0..n {
            s[i] += 0.01 * (2.0 * std::f64::consts::PI * 300.0 * i as f64 / 16_000.0).sin() as f32;
        }
        let _ = c.process(&mut s);
        let mean = s.iter().sum::<f32>() / n as f32;
        assert!(mean.abs() < 0.01, "DC not removed, mean {mean}");
    }

    #[test]
    fn hp_attenuates_80hz_but_not_200hz() {
        // At the 80 Hz cutoff a 2nd-order HP gives ≈ −3 dB vs a 200 Hz tone
        // which is well in the passband (barely touched).
        let mut c = Conditioner::new();
        let n = 16_000;
        let mut low = tone(80.0, 0.5, n, 16_000.0);
        let mut high = tone(200.0, 0.5, n, 16_000.0);
        let _ = c.process(&mut low);
        let mut c2 = Conditioner::new();
        let _ = c2.process(&mut high);
        let rms = |v: &[f32]| {
            (v.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / v.len() as f64).sqrt()
                as f32
        };
        let low_rms = rms(&low);
        let high_rms = rms(&high);
        let ratio_db = 20.0 * (low_rms / high_rms).log10();
        // The 80 Hz tone should be notably more attenuated than 200 Hz.
        // Allow the DC stage to have a small effect; the key assertion is
        // that the low tone lost more than the high tone.
        assert!(
            ratio_db < -1.0,
            "80 Hz only attenuated {ratio_db} dB relative to 200 Hz"
        );
    }

    #[test]
    fn agc_raises_quiet_speech_toward_target_without_exceeding_max_gain() {
        // Quiet speech at ~−40 dBFS peak (−43 dBFS RMS) should be raised
        // toward the −20 dBFS RMS target; the required gain (~+23 dB)
        // exceeds the +18 dB cap, so the cap is the expected result and the
        // output lands near −25 dBFS RMS — clearly above the input.
        let mut c = Conditioner::new();
        let n = 16_000;
        // −40 dBFS peak sine.
        let amp = 10f32.powf(-40.0 / 20.0);
        let mut s = tone(300.0, amp, n, 16_000.0);
        // Feed the AGC in 10 ms frames (160 samples) so it has time to
        // converge toward the target with its 50 ms attack / 500 ms release.
        // `process` takes `&mut Vec`, so each frame is moved out, processed,
        // and written back.
        // Feed the AGC in 10 ms frames (160 samples) so it has time to
        // converge toward the target with its 50 ms attack / 500 ms release.
        // `process` takes `&mut Vec`, so each frame is moved out, processed,
        // and written back.
        let mut last = ConditionStats {
            clipped: false,
            pct_clipped: 0.0,
            applied_gain_db: 0.0,
        };
        let mut i = 0;
        while i < s.len() {
            let end = (i + 160).min(s.len());
            let mut frame: Vec<f32> = s[i..end].to_vec();
            last = c.process(&mut frame);
            s[i..end].copy_from_slice(&frame);
            i += 160;
        }
        // Gain should be positive (raising) and bounded by +18 dB. The
        // release time-constant is 500 ms, so after 1 s of quiet speech the
        // gain is ~86% of the way to the +18 dB cap (1 - e^-2).
        assert!(
            last.applied_gain_db > 10.0,
            "gain too low: {} dB",
            last.applied_gain_db
        );
        assert!(
            last.applied_gain_db <= 18.0 + 1e-3,
            "gain exceeded max: {} dB",
            last.applied_gain_db
        );
        // Output RMS should be much closer to the −20 dBFS target than input.
        let rms =
            (s.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / n as f64).sqrt() as f32;
        let rms_db = 20.0 * rms.log10();
        assert!(
            rms_db > -30.0,
            "output RMS {rms_db} dBFS, expected near −25 (input was −43)"
        );
    }

    #[test]
    fn limiter_bounds_output_to_minus_1_dbfs() {
        // A loud input that would exceed the ceiling must be clamped so that
        // |s| <= 0.891 (−1 dBFS) for every output sample.
        let mut c = Conditioner::new();
        let n = 16_000;
        let mut s = tone(300.0, 1.2, n, 16_000.0); // amp 1.2 > ceiling
        let _ = c.process(&mut s);
        let ceiling = Consts::default().limiter_ceiling;
        for &x in &s {
            assert!(
                x.abs() <= ceiling + 1e-5,
                "sample {x} exceeds ceiling {ceiling}"
            );
        }
    }

    #[test]
    fn clipping_stat_flags_hot_input() {
        // A hot sine wave (peak 1.2, above the 0.99 clip threshold) must
        // report clipped=true with a high pct_clipped. The DC stage removes
        // any offset; the AGC gain is ~1.0 for a signal already near the
        // target level, so the pre-limiter peaks still exceed the threshold.
        let mut c = Conditioner::new();
        let n = 16_000;
        let mut s = tone(300.0, 1.2, n, 16_000.0);
        let stats = c.process(&mut s);
        assert!(
            stats.clipped,
            "expected clipped, got pct {}",
            stats.pct_clipped
        );
        assert!(stats.pct_clipped > 0.1, "pct_clipped {}", stats.pct_clipped);
    }

    #[test]
    fn clean_input_is_not_flagged_clipped() {
        let mut c = Conditioner::new();
        let n = 16_000;
        let mut s = tone(300.0, 0.1, n, 16_000.0);
        let stats = c.process(&mut s);
        assert!(
            !stats.clipped,
            "clean input flagged clipped, pct {}",
            stats.pct_clipped
        );
    }

    #[test]
    fn empty_input_returns_zeroed_stats() {
        let mut c = Conditioner::new();
        let mut s: Vec<f32> = Vec::new();
        let stats = c.process(&mut s);
        assert!(!stats.clipped);
        assert_eq!(stats.pct_clipped, 0.0);
        assert_eq!(stats.applied_gain_db, 0.0);
    }

    #[test]
    fn consts_documented_values() {
        let c = Consts::default();
        assert_eq!(c.dc_fc_hz, 20.0);
        assert_eq!(c.hp_fc_hz, 80.0);
        assert_eq!(c.sample_rate, 16_000.0);
        assert_eq!(c.agc_target_db, -20.0);
        assert_eq!(c.agc_max_gain_db, 18.0);
        assert!((c.agc_attack_s - 0.05).abs() < 1e-9);
        assert!((c.agc_release_s - 0.5).abs() < 1e-9);
        assert!((c.limiter_ceiling - 10f32.powf(-1.0 / 20.0)).abs() < 1e-6);
        assert_eq!(c.clip_threshold, 0.99);
        assert_eq!(c.clip_pct, 0.001);
    }

    #[test]
    fn conditioning_default_is_off() {
        // Wired-but-off: the default is false; enabling is an eval decision.
        assert!(!AUDIO_CONDITIONING_DEFAULT);
    }
}
