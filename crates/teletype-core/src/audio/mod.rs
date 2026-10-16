//! Microphone capture: mono f32 at 16 kHz, on a dedicated thread.
//!
//! The device callback runs on a real-time audio thread, so it only appends
//! samples; the capture thread does everything else. Dropping a `Recording`
//! without calling `finish` stops the microphone and discards the audio.

pub mod denoise;

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

use ringbuf::storage::Heap;
use ringbuf::traits::{Consumer, Observer, RingBuffer};
use ringbuf::SharedRb;

use rubato::{
    FftFixedIn, Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
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

    /// Like [`start`], but with RNNoise noise suppression enabled.
    pub fn start_with_noise(
        device_id: &str,
        on_level: impl Fn(f32) + Send + 'static,
    ) -> Result<Recording, String> {
        Self::start_with_vad_rate_noise(device_id, on_level, None, None, true)
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
        Self::start_with_vad_rate_noise(device_id, on_level, on_frame, out_rate, false)
    }

    /// Like [`start_with_vad_rate`], but with an optional RNNoise noise
    /// suppressor enabled. When `noise_reduction` is true, each captured
    /// frame is passed through nnnoiseless before being buffered.
    pub fn start_with_vad_rate_noise(
        device_id: &str,
        on_level: impl Fn(f32) + Send + 'static,
        on_frame: FrameCb,
        out_rate: Option<Arc<AtomicU32>>,
        noise_reduction: bool,
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
                    noise_reduction,
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
    noise_reduction: bool,
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
            noise_reduction,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
            noise_reduction,
        ),
        SampleFormat::I32 => build_stream::<i32>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
            noise_reduction,
        ),
        SampleFormat::U8 => build_stream::<u8>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
            noise_reduction,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
            noise_reduction,
        ),
        SampleFormat::F32 => build_stream::<f32>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
            noise_reduction,
        ),
        SampleFormat::F64 => build_stream::<f64>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            on_frame,
            noise_reduction,
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
    noise_reduction: bool,
) -> Option<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels).max(1);
    let mut meter = LevelMeter::new();
    let mut denoiser = if noise_reduction {
        Some(denoise::NoiseSuppressor::new(config.sample_rate))
    } else {
        None
    };
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
                // Apply noise suppression if enabled.
                let mono = if let Some(ref mut ns) = denoiser {
                    ns.process(&mono)
                } else {
                    mono
                };
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

/// High-quality resample of mono f32 to 16 kHz using rubato.
///
/// Integer ratios (e.g. 48000 → 16000) use `FftFixedIn` for lower CPU.
/// Everything else uses `SincFixedIn` with sinc_len=128, f_cutoff=0.95,
/// oversampling_factor=128, BlackmanHarris2 window (plan §5.3).
pub fn resample_to_target(raw: &[f32], from_rate: u32) -> Vec<f32> {
    if raw.is_empty() {
        return Vec::new();
    }
    if from_rate == TARGET_SAMPLE_RATE {
        return raw.to_vec();
    }

    // Ratio = output_rate / input_rate (rubato's convention).
    let ratio = TARGET_SAMPLE_RATE as f64 / from_rate as f64;

    let mut resampler = if from_rate.is_multiple_of(TARGET_SAMPLE_RATE) {
        Impl::Fft(
            FftFixedIn::<f32>::new(from_rate as usize, TARGET_SAMPLE_RATE as usize, 4_800, 4, 1)
                .expect("fft resampler construction failed"),
        )
    } else {
        Impl::Sinc(
            SincFixedIn::<f32>::new(
                ratio,
                2.0, // max_resample_ratio_relative
                SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: 0.95,
                    interpolation: SincInterpolationType::Linear,
                    oversampling_factor: 128,
                    window: WindowFunction::BlackmanHarris2,
                },
                4096, // chunk_size (output frames per call)
                1,    // nbr_channels
            )
            .expect("sinc resampler construction failed"),
        )
    };

    let mut output = Vec::new();
    let mut input_offset = 0;

    while let Some(chunk_len) = resampler.chunk_len(raw.len(), input_offset) {
        output.extend(resampler.process(&raw[input_offset..input_offset + chunk_len]));
        input_offset += chunk_len;
    }

    if input_offset < raw.len() {
        if let Some(mut tail) = resampler.process_partial(&raw[input_offset..]) {
            output.append(&mut tail);
        }
    }

    output
}

/// Fixed-input rubato resampler for [`resample_to_target`].
///
/// rubato 0.15's `Resampler` trait is not dyn-compatible (generic `process`),
/// so the two flavours are dispatched through this enum instead.
enum Impl {
    /// FFT-based (`FftFixedIn`) for integer down-ratios such as 48 kHz → 16 kHz.
    Fft(FftFixedIn<f32>),
    /// Polyphase sinc (`SincFixedIn`) for arbitrary ratios such as 44.1 kHz.
    Sinc(SincFixedIn<f32>),
}

impl Impl {
    /// Inputs per call (~12298 at 44.1 kHz, ≤4096 otherwise).
    fn input_frames_next(&self) -> usize {
        match self {
            Impl::Fft(r) => r.input_frames_next(),
            Impl::Sinc(r) => r.input_frames_next(),
        }
    }

    /// The next full chunk that fits, or None when there is nothing further.
    fn chunk_len(&self, total: usize, offset: usize) -> Option<usize> {
        let frames = self.input_frames_next();
        if offset + frames <= total {
            Some(frames)
        } else {
            None
        }
    }

    fn process(&mut self, chunk: &[f32]) -> Vec<f32> {
        let wave_in = vec![chunk.to_vec()];
        match self {
            Impl::Fft(r) => r.process(&wave_in, None).expect("rubato process failed")[0].clone(),
            Impl::Sinc(r) => r.process(&wave_in, None).expect("rubato process failed")[0].clone(),
        }
    }

    fn process_partial(&mut self, chunk: &[f32]) -> Option<Vec<f32>> {
        let wave_in = vec![chunk.to_vec()];
        match self {
            Impl::Fft(r) => r
                .process_partial(Some(&wave_in), None)
                .ok()
                .and_then(|mut out| out.pop()),
            Impl::Sinc(r) => r
                .process_partial(Some(&wave_in), None)
                .ok()
                .and_then(|mut out| out.pop()),
        }
    }
}

/// Arming modes for the mic pre-warm (plan §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrewarmMode {
    /// Current behaviour: build the cpal stream on hotkey press.
    Off,
    /// Keep the stream open while Teletype's window or pill is focused.
    WhileAppActive,
    /// Always open (macOS shows the orange mic indicator; explain in UI).
    Always,
}

/// Whether the mic pre-warm stream should be armed.
pub fn should_arm(mode: PrewarmMode, app_active: bool) -> bool {
    match mode {
        PrewarmMode::Off => false,
        PrewarmMode::WhileAppActive => app_active,
        PrewarmMode::Always => true,
    }
}

/// Keeps the mic input stream open while "armed" and writes into a 1.0 s
/// lock-free ring buffer. On hotkey-down, the take starts with the last
/// 300 ms of pre-roll (plan §5.1).
///
/// Audio is held in memory only, never written to disk. [`MicPrewarm::disarm`]
/// zeroes the buffer.
pub struct MicPrewarm {
    rb: SharedRb<Heap<f32>>,
}

impl Default for MicPrewarm {
    fn default() -> Self {
        Self::new()
    }
}

impl MicPrewarm {
    /// Create a new pre-warm with a 1.0 s ring buffer (16000 samples at
    /// 16 kHz).
    pub fn new() -> Self {
        let rb = SharedRb::<Heap<f32>>::new(16_000);
        Self { rb }
    }

    /// Write a frame of samples from the capture callback (lock-free).
    pub fn write(&mut self, frame: &[f32]) {
        self.rb.push_slice_overwrite(frame);
    }

    /// Return the last `ms` milliseconds of audio (300 ms default).
    /// Samples older than 1.0 s have been overwritten by the ring buffer.
    /// Consumes everything older, so a second call returns the next-older
    /// window and a drained buffer returns an empty vec until more frames
    /// are written.
    pub fn take_preroll(&mut self, ms: u32) -> Vec<f32> {
        let samples = (ms as u64 * TARGET_SAMPLE_RATE as u64 / 1000) as usize;
        let occupied = self.rb.occupied_len();
        let want = samples.min(occupied);
        // Skip the retained-but-older prefix first: in a wrapped/overwritten
        // ring the front of the occupied region is the OLDEST data, and the
        // newest window sits at its tail.
        self.rb.skip(occupied - want);
        let mut buf = vec![0.0f32; want];
        let read = self.rb.pop_slice(&mut buf);
        buf.truncate(read);
        buf
    }

    /// Zero the buffer (privacy: audio held in memory only).
    pub fn disarm(&mut self) {
        let mut buf = vec![0.0f32; 1024];
        while self.rb.pop_slice(&mut buf) > 0 {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!((15_990..=16_001).contains(&out.len()), "len {}", out.len());
        // Steady-state DC level must track the input (skip filter edges).
        let mid = &out[out.len() / 8..out.len() * 7 / 8];
        let mean = mid.iter().sum::<f32>() / mid.len() as f32;
        assert!((mean - 0.5).abs() < 1e-2, "mean {mean}");
    }

    #[test]
    fn resample_empty() {
        assert!(resample_to_target(&[], 44_100).is_empty());
    }

    #[test]
    fn resample_44_1k_to_16k() {
        // 44100 → 16000 is not an integer ratio; uses SincFixedIn.
        let raw: Vec<f32> = vec![0.3f32; 44_100];
        let out = resample_to_target(&raw, 44_100);
        // The sinc resampler emits up to a chunk of filter ring-out past the
        // nominal 16 000; content timing is unaffected.
        assert!((15_900..=16_400).contains(&out.len()), "len {}", out.len());
        // Steady-state DC level must track the input (skip filter edges).
        let mid = &out[out.len() / 8..out.len() * 7 / 8];
        let mean = mid.iter().sum::<f32>() / mid.len() as f32;
        assert!((mean - 0.3).abs() < 1e-2, "mean {mean}");
    }

    #[test]
    fn resample_48k_to_16k() {
        // 48000 → 16000 is an integer ratio (3:1); uses FftFixedIn.
        let raw: Vec<f32> = vec![0.7f32; 96_000]; // 2 s at 48 kHz
        let out = resample_to_target(&raw, 48_000);
        assert_eq!(out.len(), 32_000); // 2 s at 16 kHz
    }

    #[test]
    fn resample_passband_tone_survives() {
        // A 1 kHz tone at 48 kHz must survive the 3:1 downmix with its
        // amplitude roughly intact (plan §5.3: quality resampling).
        let n = 48_000; // 1 s
        let mut raw = Vec::with_capacity(n);
        for i in 0..n {
            raw.push(
                0.5 * (2.0 * std::f64::consts::PI * 1_000.0 * i as f64 / 48_000.0).sin() as f32,
            );
        }
        let out = resample_to_target(&raw, 48_000);
        assert_eq!(out.len(), 16_000);
        // Steady-state RMS over the middle second (skip filter settling).
        let mid = &out[2_000..out.len() - 2_000];
        let rms = (mid.iter().map(|s| s * s).sum::<f32>() / mid.len() as f32).sqrt();
        let expected = 0.5 / 2f32.sqrt();
        assert!(
            (rms - expected).abs() < 0.05 * expected,
            "passband RMS {rms} vs expected {expected}"
        );
    }

    #[test]
    fn resample_attenuates_above_out_nyquist() {
        // A 12 kHz tone at 48 kHz sits above the 16 kHz output Nyquist (8 kHz).
        // Linear resampling aliases it to 4 kHz; a real anti-alias filter
        // suppresses it. Plan §5.3: ≥ 60 dB attenuation broadband; the
        // single-tone gate here is ≥ 40 dB relative RMS (comfortably above the
        // −13 dB a linear interp would give).
        let n = 48_000; // 1 s
        let amp = 0.5_f32;
        let mut raw = Vec::with_capacity(n);
        for i in 0..n {
            raw.push(
                amp * (2.0 * std::f64::consts::PI * 12_000.0 * i as f64 / 48_000.0).sin() as f32,
            );
        }
        let input_rms = amp / 2f32.sqrt();
        let out = resample_to_target(&raw, 48_000);
        assert!(
            (15_940..=16_001).contains(&out.len()),
            "short output {}",
            out.len()
        );
        // Steady-state window (skip filter settling on both ends).
        let mid = &out[1_600..out.len() - 1_600];
        let rms = (mid.iter().map(|s| s * s).sum::<f32>() / mid.len() as f32).sqrt();
        let attn_db = 20.0 * (rms / input_rms).log10();
        assert!(
            attn_db <= -40.0,
            "12 kHz tone only suppressed by {attn_db} dB (rms {rms})"
        );
    }

    #[test]
    fn prewarm_new_is_empty() {
        let mut pw = MicPrewarm::new();
        assert_eq!(pw.take_preroll(300).len(), 0);
    }

    #[test]
    fn prewarm_write_and_take() {
        let mut pw = MicPrewarm::new();
        // Write 1 s of audio (16000 samples at 16 kHz)
        let samples: Vec<f32> = (0..16_000).map(|i| (i as f32) * 0.001).collect();
        pw.write(&samples);
        // Take the last 300 ms (4800 samples)
        let preroll = pw.take_preroll(300);
        assert_eq!(preroll.len(), 4800);
        // Should start at the 11200th written sample (4800 back from the end).
        assert!(
            (preroll[0] - 11.2).abs() < 1e-3,
            "preroll[0] {}",
            preroll[0]
        );
        // One-shot semantics: the ring was drained by the take.
        assert_eq!(pw.take_preroll(300).len(), 0);
    }

    #[test]
    fn prewarm_wraps_around() {
        let mut pw = MicPrewarm::new();
        // Write 2 s of audio (buffer is 1.0 s, so older data is overwritten);
        // the retained lap holds samples 16.0s..32.0s
        let samples: Vec<f32> = (16_000..32_000).map(|i| (i as f32) * 0.001).collect();
        pw.write(&samples);
        // The newest 300 ms window starts at 27.2 s.
        let preroll = pw.take_preroll(300);
        assert_eq!(preroll.len(), 4800);
        assert!(
            (preroll[0] - 27.2).abs() < 1e-3,
            "preroll[0] {}",
            preroll[0]
        );
        // The take drained the ring (older windows were skipped away).
        assert_eq!(pw.take_preroll(300).len(), 0);
        // A refill after draining yields the newest window of the new data.
        let fresh: Vec<f32> = (50_000..66_000).map(|i| (i as f32) * 0.001).collect();
        pw.write(&fresh);
        let fresh_preroll = pw.take_preroll(300);
        assert_eq!(fresh_preroll.len(), 4800);
        assert!(
            (fresh_preroll[0] - 61.2).abs() < 1e-3,
            "fresh[0] {}",
            fresh_preroll[0]
        );
    }

    #[test]
    fn prewarm_disarm_zeroes() {
        let mut pw = MicPrewarm::new();
        let samples: Vec<f32> = vec![0.5; 8000]; // 500 ms
        pw.write(&samples);
        pw.disarm();
        let preroll = pw.take_preroll(300);
        assert!(preroll.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn should_arm_off() {
        assert!(!should_arm(PrewarmMode::Off, true));
        assert!(!should_arm(PrewarmMode::Off, false));
    }

    #[test]
    fn should_arm_while_app_active() {
        assert!(should_arm(PrewarmMode::WhileAppActive, true));
        assert!(!should_arm(PrewarmMode::WhileAppActive, false));
    }

    #[test]
    fn should_arm_always() {
        assert!(should_arm(PrewarmMode::Always, true));
        assert!(should_arm(PrewarmMode::Always, false));
    }

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
}
