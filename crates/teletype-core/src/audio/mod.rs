//! Microphone capture: mono f32 at 16 kHz, on a dedicated thread.
//!
//! The device callback runs on a real-time audio thread, so it only appends
//! samples; the capture thread does everything else. Dropping a `Recording`
//! without calling `finish` stops the microphone and discards the audio.

use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc::{self, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample, SampleFormat, SizedSample,
};

/// Whisper and most local STT engines expect 16 kHz mono.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Seconds of audio to reserve up front so the callback rarely grows the buffer.
const RESERVE_SECS: usize = 30;
/// How often the level meter is updated.
const LEVEL_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn list_input_devices() -> Vec<InputDevice> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|d| d.id().ok())
        .map(|id| id.to_string());
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            let name = device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| id.clone());
            Some(InputDevice {
                is_default: default_id.as_deref() == Some(id.as_str()),
                id,
                name,
            })
        })
        .collect()
}

/// Mono samples at [`TARGET_SAMPLE_RATE`].
#[derive(Debug, Clone)]
pub struct Captured {
    pub samples: Vec<f32>,
    pub duration_secs: f64,
    /// Peak amplitude (0..1) of the captured audio. Very low peaks mean the
    /// mic probably picked up no speech, so callers can distinguish
    /// "silence" from "the model found no words in real speech".
    pub peak: f32,
}

/// A running microphone capture.
pub struct Recording {
    stop_tx: Sender<()>,
    done_rx: mpsc::Receiver<Captured>,
    /// Shared live buffer the capture thread appends to, exposed so a
    /// live-preview consumer can poll audio-so-far without stopping the take.
    live: Option<(Buf, Arc<AtomicU32>)>,
}

impl Recording {
    /// Starts recording from `device_id`, or the system default when empty.
    /// `on_level` receives smoothed loudness 0..1 on the capture thread.
    pub fn start(
        device_id: &str,
        on_level: impl Fn(f32) + Send + 'static,
    ) -> Result<Recording, String> {
        Self::start_with_vad(device_id, on_level, None)
    }

    /// Like [`start`], but also feeds each captured mono frame to `on_frame`
    /// on the capture thread, in arrival order, at the device's native sample
    /// rate. Used to drive VAD auto-stop. The callback must be cheap and
    /// non-blocking (it runs on the real-time audio path's consumer thread).
    ///
    /// `on_frame` is boxed so callers can pass closures that move captured
    /// state, without the extra generic-lifetime friction of a `Fn` bound.
    pub fn start_with_vad(
        device_id: &str,
        on_level: impl Fn(f32) + Send + 'static,
        on_frame: FrameCb,
    ) -> Result<Recording, String> {
        Self::start_with_vad_rate(device_id, on_level, on_frame, None)
    }

    /// Like [`start_with_vad`], but also publishes the device's native sample
    /// rate to `out_rate` (if given) before the capture starts, so a VAD
    /// consumer can time its silence window against the real feed rate.
    pub fn start_with_vad_rate(
        device_id: &str,
        on_level: impl Fn(f32) + Send + 'static,
        on_frame: FrameCb,
        out_rate: Option<Arc<AtomicU32>>,
    ) -> Result<Recording, String> {
        let device_id = device_id.to_string();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let (done_tx, done_rx) = mpsc::channel::<Captured>();
        // Shared live buffer + rate, created here so `Recording` can hand a
        // clone to a live-preview consumer. The capture thread appends raw
        // mono samples to `buffer` and latches the device rate into `rate`.
        let buffer: Buf = Arc::new(Mutex::new(Vec::new()));
        let rate: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));

        let buffer_for_thread = Arc::clone(&buffer);
        let rate_for_thread = Arc::clone(&rate);
        let started = thread::Builder::new()
            .name("teletype-audio".into())
            .spawn(move || {
                run_capture(
                    &device_id,
                    stop_rx,
                    ready_tx,
                    done_tx,
                    on_level,
                    on_frame,
                    out_rate,
                    buffer_for_thread,
                    rate_for_thread,
                );
            });
        let started = started.map_err(|e| format!("Couldn't start audio: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(_)) => Ok(Recording {
                stop_tx,
                done_rx,
                live: Some((buffer.clone(), rate.clone())),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                let _ = started.join();
                Err("Audio thread died before the device opened".into())
            }
        }
    }

    /// Stops the microphone and waits for the resampled audio.
    pub fn finish(self) -> Result<Captured, String> {
        let _ = self.stop_tx.send(());
        let captured = self
            .done_rx
            .recv()
            .map_err(|_| "Audio capture ended unexpectedly".to_string())?;
        Ok(captured)
    }

    /// A handle to the in-flight audio, so a live-preview consumer can poll
    /// audio-so-far without stopping the take. `None` if the capture never
    /// started. The returned clone observes the same buffer the capture
    /// thread appends to, and the same atomic holding the device's native
    /// sample rate (latched once the stream opens).
    pub fn live_preview(&self) -> Option<(Buf, Arc<AtomicU32>)> {
        self.live.clone()
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

/// Runs the capture thread until `stop_rx` fires.
///
/// Nine parameters, which is over clippy's threshold. They are a single
/// pipeline (device -> callback -> buffers -> rate negotiation) and grouping
/// them into structs would mean threading two holders through every call
/// without removing a parameter, so the allow is deliberate rather than a
/// workaround. If a tenth arrives, group instead of bumping this.
#[allow(clippy::too_many_arguments)]
fn run_capture(
    device_id: &str,
    stop_rx: mpsc::Receiver<()>,
    ready_tx: Sender<Result<u32, String>>,
    done_tx: Sender<Captured>,
    on_level: impl Fn(f32) + Send,
    on_frame: FrameCb,
    out_rate: Option<Arc<AtomicU32>>,
    buffer: Buf,
    rate: Arc<AtomicU32>,
) {
    let host = cpal::default_host();
    let device = if device_id.is_empty() {
        host.default_input_device()
    } else {
        // Try the selected device first; fall back to the system default
        // when it is no longer connected (unplugged, renamed, etc.).
        let selected = host.input_devices().ok().and_then(|devices| {
            devices
                .into_iter()
                .find(|d| d.id().ok().map(|id| id.to_string()).as_deref() == Some(device_id))
        });
        match selected {
            Some(d) => Some(d),
            None => {
                tracing::warn!(
                    device_id,
                    "selected input device not found; falling back to system default"
                );
                host.default_input_device()
            }
        }
    };
    let Some(device) = device else {
        // No device at all: give the user actionable info.
        let count = host.input_devices().map(|d| d.count()).unwrap_or(0);
        let msg = if count == 0 {
            "No microphone found. Connect a microphone or check System Settings > Sound > Input."
                .to_string()
        } else {
            format!("No usable input device (found {count} device(s) but none could be opened).")
        };
        let _ = ready_tx.send(Err(msg));
        return;
    };

    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("No input config: {e}")));
            return;
        }
    };
    let sample_rate = config.sample_rate();
    // Publish the rate before the stream is built/played, so the VAD consumer
    // never sees a frame with the rate still unset (no race with the first
    // callback).
    if let Some(slot) = out_rate.as_ref() {
        slot.store(sample_rate, Ordering::Relaxed);
    }
    let _channels = usize::from(config.channels()).max(1);
    let format = config.sample_format();
    let stream_config = config.config();

    // Reserve capacity (not length) so the callback rarely reallocates, while
    // keeping the vec empty so only real audio is captured.
    buffer
        .lock()
        .map(|mut b| b.reserve((sample_rate as usize) * RESERVE_SECS))
        .ok();
    let level: Arc<AtomicU32> = Arc::new(AtomicU32::new(NO_LEVEL));

    // Latch the device's native rate into the shared atomic so a live-preview
    // consumer can resample the buffer to 16 kHz without knowing the device.
    rate.store(sample_rate, Ordering::Relaxed);

    let buf_clone = Arc::clone(&buffer);
    let level_clone = Arc::clone(&level);

    let err_fn = Box::new(|err| eprintln!("[audio] stream error: {err}"));
    let stream = match format {
        SampleFormat::I8 => build_stream::<i8>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
        ),
        SampleFormat::I32 => build_stream::<i32>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
        ),
        SampleFormat::U8 => build_stream::<u8>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
        ),
        SampleFormat::F32 => build_stream::<f32>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
        ),
        SampleFormat::F64 => build_stream::<f64>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
        ),
        other => {
            let _ = ready_tx.send(Err(format!("Unsupported sample format {other:?}")));
            return;
        }
    };
    let Some(stream) = stream else {
        return;
    };

    if let Err(e) = stream.play() {
        let _ = ready_tx.send(Err(format!("Couldn't start microphone: {e}")));
        return;
    }
    let _ = ready_tx.send(Ok(sample_rate));

    loop {
        match stop_rx.recv_timeout(LEVEL_INTERVAL / 2) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                let bits = level.swap(NO_LEVEL, Ordering::Relaxed);
                if bits != NO_LEVEL {
                    on_level(f32::from_bits(bits));
                }
            }
        }
    }

    let raw = buffer.lock().map(|b| b.clone()).unwrap_or_default();
    let resampled = resample_to_target(&raw, sample_rate);
    let duration_secs = resampled.len() as f64 / TARGET_SAMPLE_RATE as f64;
    let peak = resampled.iter().fold(0f32, |p, &s| p.max(s.abs()));
    let _ = done_tx.send(Captured {
        peak,
        samples: resampled,
        duration_secs,
    });
}

pub type Buf = Arc<Mutex<Vec<f32>>>;

pub type Lvl = Arc<AtomicU32>;

/// Hook fed each captured mono frame (drives VAD auto-stop).
type FrameCb = Option<Box<dyn Fn(&[f32]) + Send + 'static>>;

/// Marks the shared level as "no new level yet". Real levels are in 0..1.
const NO_LEVEL: u32 = u32::MAX;

/// Turns incoming audio into a steady 0..1 loudness for the pill's waveform:
/// loudness in decibels (not raw amplitude), a noise gate so room noise stays
/// flat, and smoothing that rises fast and falls slowly.
struct LevelMeter {
    sum_squares: f64,
    count: usize,
    peak: f32,
    smoothed: f32,
    last_sent: Instant,
}

impl LevelMeter {
    /// Quietest level shown, in dBFS. Everything below reads as silence.
    const FLOOR_DB: f32 = -58.0;
    /// Share of the scale treated as background noise and cut off.
    const NOISE_GATE: f32 = 0.25;
    const RISE: f32 = 0.55;
    const FALL: f32 = 0.18;

    fn new() -> Self {
        Self {
            sum_squares: 0.0,
            count: 0,
            peak: 0.0,
            smoothed: 0.0,
            last_sent: Instant::now(),
        }
    }

    /// Adds samples, and returns a new smoothed level once per `LEVEL_INTERVAL`.
    fn push(&mut self, samples: &[f32]) -> Option<f32> {
        for s in samples {
            self.sum_squares += f64::from(*s) * f64::from(*s);
            self.peak = self.peak.max(s.abs());
        }
        self.count += samples.len();
        if self.last_sent.elapsed() < LEVEL_INTERVAL || self.count == 0 {
            return None;
        }

        let rms = (self.sum_squares / self.count as f64).sqrt() as f32;
        let level = level_from_amplitudes(rms, self.peak);
        let rate = if level > self.smoothed {
            Self::RISE
        } else {
            Self::FALL
        };
        self.smoothed += (level - self.smoothed) * rate;

        self.sum_squares = 0.0;
        self.count = 0;
        self.peak = 0.0;
        self.last_sent = Instant::now();
        Some(self.smoothed)
    }
}

/// Maps RMS and peak amplitude (0..1) to a gated 0..1 display level.
fn level_from_amplitudes(rms: f32, peak: f32) -> f32 {
    if rms.is_nan() || peak.is_nan() {
        return 0.0;
    }
    let normalize = |amplitude: f32| {
        let db = 20.0 * amplitude.max(1e-5).log10();
        ((db - LevelMeter::FLOOR_DB) / -LevelMeter::FLOOR_DB).clamp(0.0, 1.0)
    };
    let level = (normalize(rms) * 0.8).max(normalize(peak));
    ((level - LevelMeter::NOISE_GATE) / (1.0 - LevelMeter::NOISE_GATE)).clamp(0.0, 1.0)
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: Buf,
    level: Lvl,
    err_fn: Box<dyn Fn(cpal::Error) + Send + 'static>,
    on_frame: FrameCb,
) -> Option<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels).max(1);
    let mut meter = LevelMeter::new();
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &_| {
                // Downmix to mono by averaging each frame's channels.
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| {
                        frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>()
                            / frame.len().max(1) as f32
                    })
                    .collect();
                if let Ok(mut buf) = buffer.lock() {
                    let start = buf.len();
                    buf.extend(mono.iter().copied());
                    if let Some(l) = meter.push(&buf[start..]) {
                        level.store(l.to_bits(), Ordering::Relaxed);
                    }
                }
                // VAD hook: fire after the buffer lock is released so the
                // capture thread never waits on the detector.
                if let Some(cb) = on_frame.as_ref() {
                    cb(&mono);
                }
            },
            err_fn,
            None,
        )
        .ok()
}

/// Linear-interpolation resample of mono f32 to 16 kHz.
pub fn resample_to_target(raw: &[f32], from_rate: u32) -> Vec<f32> {
    if raw.is_empty() {
        return Vec::new();
    }
    if from_rate == TARGET_SAMPLE_RATE {
        return raw.to_vec();
    }
    let ratio = from_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let out_len = (raw.len() as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        let a = raw[idx.min(raw.len() - 1)];
        let b = raw[(idx + 1).min(raw.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_is_flat_for_silence_and_noise_and_high_for_speech() {
        assert_eq!(level_from_amplitudes(0.0, 0.0), 0.0);
        // Room noise around -50 dBFS stays below the gate.
        assert_eq!(level_from_amplitudes(0.002, 0.004), 0.0);
        // Normal speech around -20 dBFS peaks shows clearly.
        let speech = level_from_amplitudes(0.03, 0.1);
        assert!(speech > 0.4 && speech < 0.8, "speech level {speech}");
        assert_eq!(level_from_amplitudes(1.0, 1.0), 1.0);
    }

    #[test]
    fn level_ignores_nan_samples() {
        assert_eq!(level_from_amplitudes(f32::NAN, f32::NAN), 0.0);
    }

    #[test]
    fn meter_waits_for_its_interval_and_stays_in_range() {
        let mut meter = LevelMeter::new();
        assert_eq!(meter.push(&[1.0; 480]), None);
        meter.last_sent -= LEVEL_INTERVAL;
        let level = meter.push(&[1.0; 480]).expect("interval elapsed");
        assert!((0.0..=1.0).contains(&level), "level {level}");
        // A loud burst rises toward full scale but is smoothed.
        assert!(level < 1.0);
        // Nothing new to measure.
        meter.last_sent -= LEVEL_INTERVAL;
        assert_eq!(meter.push(&[]), None);
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let raw: Vec<f32> = (0..100).map(|i| i as f32 * 0.01).collect();
        let out = resample_to_target(&raw, TARGET_SAMPLE_RATE);
        assert_eq!(out.len(), raw.len());
        for (a, b) in raw.iter().zip(&out) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    #[test]
    fn resample_down_to_16k() {
        let raw: Vec<f32> = vec![0.5f32; 48_000];
        let out = resample_to_target(&raw, 48_000);
        assert_eq!(out.len(), 16_000);
        assert!((out[0] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn resample_empty() {
        assert!(resample_to_target(&[], 44_100).is_empty());
    }
}
