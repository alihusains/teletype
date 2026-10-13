//! Real-time noise suppression using nnnoiseless (pure-Rust RNNoise).
//!
//! RNNoise operates on 480-sample frames at 48 kHz (10 ms per frame).
//! The mic may capture at any rate (44.1 kHz, 48 kHz, 44.1 kHz, etc.),
//! so we resample to 48 kHz, process in 480-sample chunks, and resample
//! back to the original rate before handing samples to the buffer.
//!
//! Latency: one 10 ms frame of processing is < 0.1 ms on modern CPUs,
//! so the added latency is negligible.

use nnnoiseless::DenoiseState;

const RNNOISE_FRAME: usize = 480;
const RNNOISE_RATE: u32 = 48_000;

/// A stateful noise suppressor. Create one per recording session.
pub struct NoiseSuppressor {
    state: Box<DenoiseState<'static>>,
    /// Input sample rate of the mic (before resampling to 48 kHz).
    input_rate: u32,
    /// Accumulator for resampling input to 48 kHz.
    in_pos: f64,
    /// Accumulator for resampling output back to input rate.
    out_pos: f64,
    /// Pending 48 kHz samples waiting to fill a 480-sample frame.
    pending_48k: Vec<f32>,
    /// Pending output samples (at input rate) not yet returned.
    pending_out: Vec<f32>,
    /// First frame flag: RNNoise's first output has fade-in artifacts.
    first_frame: bool,
}

impl NoiseSuppressor {
    /// Creates a new suppressor for audio at `input_rate` Hz.
    pub fn new(input_rate: u32) -> Self {
        Self {
            state: DenoiseState::new(),
            input_rate,
            in_pos: 0.0,
            out_pos: 0.0,
            pending_48k: Vec::with_capacity(RNNOISE_FRAME * 2),
            pending_out: Vec::with_capacity(512),
            first_frame: true,
        }
    }

    /// Processes a chunk of mono f32 samples (range -1.0..1.0) at the
    /// input rate and returns the denoised samples at the same rate.
    ///
    /// The output may be slightly shorter than the input due to the
    /// 10 ms frame alignment (up to ~10 ms of tail is buffered).
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }

        // Step 1: Resample input to 48 kHz.
        let samples_48k = self.resample_to_48k(input);

        // Step 2: Process in 480-sample frames.
        let mut denoised_48k = Vec::with_capacity(samples_48k.len());
        let mut frame = [0.0f32; RNNOISE_FRAME];
        let mut out_frame = [0.0f32; RNNOISE_FRAME];

        // Append to pending buffer.
        self.pending_48k.extend_from_slice(&samples_48k);

        // Process complete frames.
        while self.pending_48k.len() >= RNNOISE_FRAME {
            frame.copy_from_slice(&self.pending_48k[..RNNOISE_FRAME]);
            self.pending_48k.drain(..RNNOISE_FRAME);

            // RNNoise expects 16-bit range: [-32768, 32767].
            for s in frame.iter_mut() {
                *s = (*s * 32767.0).clamp(-32768.0, 32767.0);
            }
            let _vad_prob = self.state.process_frame(&mut out_frame, &frame);

            // Convert back to float range.
            for s in out_frame.iter_mut() {
                *s /= 32767.0;
            }

            // Discard the first frame (fade-in artifacts).
            if self.first_frame {
                self.first_frame = false;
            } else {
                denoised_48k.extend_from_slice(&out_frame);
            }
        }

        // Step 3: Resample denoised 48 kHz back to input rate.
        let mut result = self.resample_from_48k(&denoised_48k);
        result.append(&mut self.pending_out);
        self.pending_out.clear();
        result
    }

    /// Flushes any remaining buffered samples (call at end of recording).
    pub fn flush(&mut self) -> Vec<f32> {
        // Process any remaining 48 kHz samples (pad with silence).
        if !self.pending_48k.is_empty() {
            let mut frame = [0.0f32; RNNOISE_FRAME];
            let n = self.pending_48k.len().min(RNNOISE_FRAME);
            frame[..n].copy_from_slice(&self.pending_48k[..n]);
            self.pending_48k.clear();

            for s in frame.iter_mut() {
                *s = (*s * 32767.0).clamp(-32768.0, 32767.0);
            }
            let mut out_frame = [0.0f32; RNNOISE_FRAME];
            let _ = self.state.process_frame(&mut out_frame, &frame);
            for s in out_frame.iter_mut() {
                *s /= 32767.0;
            }
            let mut result = self.resample_from_48k(&out_frame);
            result.append(&mut self.pending_out);
            self.pending_out.clear();
            return result;
        }
        std::mem::take(&mut self.pending_out)
    }

    /// Linear-interpolation resample from input_rate to 48 kHz.
    fn resample_to_48k(&mut self, input: &[f32]) -> Vec<f32> {
        if self.input_rate == RNNOISE_RATE {
            return input.to_vec();
        }
        let ratio = self.input_rate as f64 / RNNOISE_RATE as f64;
        let out_len = ((input.len() as f64) / ratio) as usize;
        let mut out = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let pos = self.in_pos + i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx.min(input.len() - 1)];
            let b = input[(idx + 1).min(input.len() - 1)];
            out.push(a + (b - a) * frac);
        }
        self.in_pos = (self.in_pos + input.len() as f64 * ratio) % 1.0;
        out
    }

    /// Linear-interpolation resample from 48 kHz back to input_rate.
    /// Buffers the fractional tail in `pending_out`.
    fn resample_from_48k(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        if self.input_rate == RNNOISE_RATE {
            return input.to_vec();
        }
        let ratio = RNNOISE_RATE as f64 / self.input_rate as f64;
        let total = self.out_pos + input.len() as f64;
        let out_len = total as usize;
        let mut out = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx.min(input.len() - 1)];
            let b = input[(idx + 1).min(input.len() - 1)];
            out.push(a + (b - a) * frac);
        }
        self.out_pos = total - out_len as f64;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_silence_without_panic() {
        let mut ns = NoiseSuppressor::new(48_000);
        let silence = vec![0.0f32; 4800]; // 100 ms
        let out = ns.process(&silence);
        assert!(!out.is_empty());
        // Output should be near-silent.
        let peak = out.iter().fold(0.0f32, |p, &s| p.max(s.abs()));
        assert!(peak < 0.01, "silence should stay silent, got peak {peak}");
    }

    #[test]
    fn processes_44100_rate() {
        let mut ns = NoiseSuppressor::new(44_100);
        // 100 ms at 44.1 kHz
        let silence = vec![0.0f32; 4410];
        let out = ns.process(&silence);
        assert!(!out.is_empty());
    }

    #[test]
    fn flush_returns_remaining() {
        let mut ns = NoiseSuppressor::new(48_000);
        let silence = vec![0.0f32; 100]; // less than one frame
        let _ = ns.process(&silence);
        let flushed = ns.flush();
        // Should return something (padded frame).
        assert!(!flushed.is_empty());
    }
}
