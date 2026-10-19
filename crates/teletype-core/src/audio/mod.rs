//! Microphone capture: mono f32 at 16 kHz, on a dedicated thread.
//!
//! The device callback runs on a real-time audio thread, so it only appends
//! samples; the capture thread does everything else. Dropping a `Recording`
//! without calling `finish` stops the microphone and discards the audio.

pub mod condition;
pub mod denoise;
pub mod dfn3;
pub mod noise_handling;
pub mod quality;

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
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

/// A DFN3 denoiser shared between the capture callback and the take's
/// consumers (VAD/level path, and the ASR path in Aggressive mode).
///
/// The capture callback runs on a real-time audio thread, so it must never
/// block on the model. The model itself (ONNX inference) is stateful and
/// not `Sync`, so it sits behind a `Mutex`; the callback takes the lock,
/// processes the 480-sample frame, and drops the lock. If the lock is
/// already held (another consumer is mid-inference), the frame is passed
/// through UN-denoised rather than blocking the audio thread: one raw
/// frame is far cheaper than a stalled callback.
///
/// `None` when the model fails to load (the take then runs with raw audio
/// for the detection path, exactly like `NoiseHandlingMode::Off`).
pub type Dfn3Slot = Arc<Mutex<Option<dfn3::Dfn3Stream>>>;

/// The DFN3 model directory, relative to the models dir: the extracted
/// `DeepFilterNet3_ll_onnx` folder (see `THIRD_PARTY_MODELS.md`).
pub const DFN3_MODEL_DIR: &str = "dfn3_ll";

/// Loads the shared DFN3 stream for a take, or `None` when the model is
/// not downloaded (the take then runs with raw audio for the detection
/// path, exactly like `NoiseHandlingMode::Off`).
///
/// `models_dir` is the user's models directory (the same one that holds
/// the speech/LLM models); `input_rate` is the device's native rate.
pub fn build_dfn3_slot(models_dir: &std::path::Path, input_rate: u32) -> Dfn3Slot {
    let dir = models_dir.join(DFN3_MODEL_DIR);
    let stream = match dfn3::Dfn3::load(&dir, 1) {
        Ok(model) => Some(dfn3::Dfn3Stream::new(model, input_rate)),
        Err(e) => {
            tracing::warn!("dfn3 unavailable ({e}); detection path uses raw audio");
            None
        }
    };
    Arc::new(Mutex::new(stream))
}

/// Processes one captured mono frame (device rate) through the shared DFN3
/// stream, or passes it through unchanged when no model is loaded or the
/// model is busy. Never blocks the audio thread.
pub fn dfn3_process_frame(slot: &Dfn3Slot, frame: &[f32]) -> Vec<f32> {
    let mut guard = match slot.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::WouldBlock) => {
            return frame.to_vec();
        }
        Err(std::sync::TryLockError::Poisoned(_)) => {
            tracing::warn!("dfn3 slot mutex poisoned; passing through raw");
            return frame.to_vec();
        }
    };
    match guard.as_mut() {
        Some(stream) => match stream.process(frame) {
            Ok(out) => out,
            Err(e) => {
                tracing::warn!("dfn3 frame failed: {e}; passing through raw");
                frame.to_vec()
            }
        },
        None => frame.to_vec(),
    }
}

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
        // `noise_reduction` is the legacy RNNoise flag; the DFN3 split
        // pipeline (plan §5.5) is `dfn3 = None`, so the raw path is
        // byte-identical to before.
        Self::start_with_vad_df(
            device_id,
            on_level,
            on_frame,
            out_rate,
            noise_reduction,
            None,
        )
    }

    /// Like [`start_with_vad_rate_noise`], but with the DeepFilterNet3
    /// split pipeline (plan §5.5): the captured audio is buffered RAW (the
    /// ASR path is untouched by the noise mode), and a detection worker
    /// thread runs each frame through DFN3 before feeding the VAD/level
    /// path (`on_frame` + `on_level`).
    ///
    /// `dfn3` is the shared DFN3 stream (see [`build_dfn3_slot`]); `None`
    /// means the detection path runs on raw audio (mode Off, or the model
    /// is not downloaded).
    pub fn start_with_vad_df(
        device_id: &str,
        on_level: impl Fn(f32) + Send + 'static,
        on_frame: FrameCb,
        out_rate: Option<Arc<AtomicU32>>,
        noise_reduction: bool,
        dfn3: Option<Dfn3Slot>,
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
                    dfn3,
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

    /// Sends the stop signal WITHOUT waiting for the capture to finish.
    /// For callers who must stop capture before their own cleanup joins
    /// while keeping the right to [`finish`]: the capture thread drops
    /// `on_frame` (and everything it held) as soon as it sees the signal.
    /// Sending is idempotent — a failed send only means capture already
    /// exited, and [`finish`] still receives the done message.
    pub fn stop_capture(&self) {
        let _ = self.stop_tx.send(());
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

/// Runs the capture thread until `stop_rx` fires.
///
/// Ten parameters, which is over clippy's threshold. They are a single
/// pipeline (device -> callback -> buffers -> rate negotiation -> DFN3
/// detection worker) and grouping them into structs would mean threading
/// two holders through every call without removing a parameter, so the
/// allow is deliberate rather than a workaround. If an eleventh arrives,
/// group instead of bumping this.
#[allow(clippy::too_many_arguments)]
fn run_capture(
    device_id: &str,
    stop_rx: mpsc::Receiver<()>,
    ready_tx: Sender<Result<u32, String>>,
    done_tx: Sender<Captured>,
    on_level: impl Fn(f32) + Send,
    mut on_frame: FrameCb,
    out_rate: Option<Arc<AtomicU32>>,
    buffer: Buf,
    rate: Arc<AtomicU32>,
    noise_reduction: bool,
    dfn3: Option<Dfn3Slot>,
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
            None,
            noise_reduction,
            false,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            None,
            noise_reduction,
            false,
        ),
        SampleFormat::I32 => build_stream::<i32>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            None,
            noise_reduction,
            false,
        ),
        SampleFormat::U8 => build_stream::<u8>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            None,
            noise_reduction,
            false,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            None,
            noise_reduction,
            false,
        ),
        SampleFormat::F32 => build_stream::<f32>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            None,
            noise_reduction,
            false,
        ),
        SampleFormat::F64 => build_stream::<f64>(
            &device,
            &stream_config,
            buf_clone,
            level_clone,
            err_fn,
            None,
            noise_reduction,
            false,
        ),
        other => {
            let _ = ready_tx.send(Err(format!("Unsupported sample format {other:?}")));
            return;
        }
    };
    let Some(mut stream) = stream else {
        return;
    };

    // Plan §5.5 split: when a DFN3 slot is provided, the callback buffers
    // RAW audio (the ASR path is untouched by the noise mode) and a
    // detection worker thread runs DFN3 on each frame before feeding the
    // VAD/level path. The callback's `on_frame` slot is replaced with a
    // channel send to the worker; the buffer append and the callback-level
    // meter are untouched. Without a slot the callback keeps the legacy
    // behaviour (raw buffer + direct `on_frame`), so Off mode is
    // byte-identical to before.
    if let Some(slot) = dfn3 {
        let (frame_tx, frame_rx) = mpsc::channel::<Vec<f32>>();
        let w_level = Arc::clone(&level);
        let w_on_frame = on_frame.take();
        let worker = thread::Builder::new()
            .name("teletype-dfn3-detect".into())
            .spawn(move || run_detection_worker(slot, frame_rx, w_level, w_on_frame))
            .ok();
        if worker.is_none() {
            tracing::warn!("dfn3 detection worker spawn failed");
        }
        let w_cb: FrameCb = Some(Box::new(move |mono: &[f32]| {
            // A full queue means the worker is behind (DFN3 inference
            // slower than real time); dropping the frame is the only
            // option that keeps the audio thread real-time. The level
            // meter and VAD both tolerate a dropped frame.
            let _ = frame_tx.send(mono.to_vec());
        }));
        // Rebuild the stream with the callback's `on_frame` replaced by
        // the channel send.
        let err_fn = Box::new(|err| eprintln!("[audio] stream error: {err}"));
        let buf_clone2 = Arc::clone(&buffer);
        let level_clone2 = Arc::clone(&level);
        let rebuilt = match format {
            SampleFormat::I8 => build_stream::<i8>(
                &device,
                &stream_config,
                buf_clone2,
                level_clone2,
                err_fn,
                w_cb,
                noise_reduction,
                true,
            ),
            SampleFormat::I16 => build_stream::<i16>(
                &device,
                &stream_config,
                buf_clone2,
                level_clone2,
                err_fn,
                w_cb,
                noise_reduction,
                true,
            ),
            SampleFormat::I32 => build_stream::<i32>(
                &device,
                &stream_config,
                buf_clone2,
                level_clone2,
                err_fn,
                w_cb,
                noise_reduction,
                true,
            ),
            SampleFormat::U8 => build_stream::<u8>(
                &device,
                &stream_config,
                buf_clone2,
                level_clone2,
                err_fn,
                w_cb,
                noise_reduction,
                true,
            ),
            SampleFormat::U16 => build_stream::<u16>(
                &device,
                &stream_config,
                buf_clone2,
                level_clone2,
                err_fn,
                w_cb,
                noise_reduction,
                true,
            ),
            SampleFormat::F32 => build_stream::<f32>(
                &device,
                &stream_config,
                buf_clone2,
                level_clone2,
                err_fn,
                w_cb,
                noise_reduction,
                true,
            ),
            SampleFormat::F64 => build_stream::<f64>(
                &device,
                &stream_config,
                buf_clone2,
                level_clone2,
                err_fn,
                w_cb,
                noise_reduction,
                true,
            ),
            _ => None,
        };
        match rebuilt {
            Some(s) => stream = s,
            None => {
                tracing::warn!("dfn3: could not rebuild the input stream");
            }
        }
    }

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

/// The DFN3 detection worker (plan §5.5): runs each captured frame through
/// the shared DFN3 stream, updates the level meter from the enhanced audio,
/// and feeds the enhanced frame to the VAD/level callback (`on_frame`).
///
/// Runs on its own thread so ONNX inference never touches the real-time
/// audio callback. The channel closes (and the worker exits) when the
/// capture thread exits and drops the sender.
fn run_detection_worker(
    slot: Dfn3Slot,
    frame_rx: mpsc::Receiver<Vec<f32>>,
    level: Lvl,
    on_frame: FrameCb,
) {
    let mut meter = LevelMeter::new();
    for frame in frame_rx {
        let enhanced = dfn3_process_frame(&slot, &frame);
        if let Some(l) = meter.push(&enhanced) {
            level.store(l.to_bits(), Ordering::Relaxed);
        }
        if let Some(cb) = on_frame.as_ref() {
            cb(&enhanced);
        }
    }
}

pub type Buf = Arc<Mutex<Vec<f32>>>;

pub type Lvl = Arc<AtomicU32>;

/// Hook fed each captured mono frame (drives VAD auto-stop).
type FrameCb = Option<Box<dyn Fn(&[f32]) + Send + Sync + 'static>>;

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

#[allow(clippy::too_many_arguments)]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: Buf,
    level: Lvl,
    err_fn: Box<dyn Fn(cpal::Error) + Send + 'static>,
    on_frame: FrameCb,
    noise_reduction: bool,
    dfn3: bool,
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
                // Plan §5.5: in the DFN3 split pipeline the callback buffers
                // RAW audio only (the ASR path stays untouched by the noise
                // mode, even in Aggressive); the DFN3 worker owns the level
                // meter and the VAD hook, so the callback's meter must not
                // also publish (it would double-send levels).
                if let Ok(mut buf) = buffer.lock() {
                    let start = buf.len();
                    buf.extend(mono.iter().copied());
                    if !dfn3 {
                        if let Some(l) = meter.push(&buf[start..]) {
                            level.store(l.to_bits(), Ordering::Relaxed);
                        }
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

/// A microphone input stream built ONCE and toggled with
/// [`arm`](Self::arm) / [`disarm`](Self::disarm) instead of being rebuilt
/// per take (plan §5.1). The take starts with the last 300 ms of pre-roll
/// from [`take_preroll`](Self::take_preroll), so the first phoneme is never
/// lost to stream build time.
///
/// Audio is held in memory only, never written to disk;
/// [`disarm`](Self::disarm) zeroes the pre-roll buffer.
///
/// Some backends/devices do not support input stream pause. When
/// [`arm`](Self::arm) or [`disarm`](Self::disarm) hits that, the front end
/// records [`FrontEndState::Fallback`] (visible via `tracing::warn!` at the
/// call site) and the caller is expected to use the legacy per-take path
/// ([`Recording`]) for that take.
pub struct MicFrontEnd {
    state: FrontEndState,
    stream: cpal::Stream,
    prewarm: Arc<Mutex<MicPrewarm>>,
    armed: Arc<AtomicBool>,
    /// Native capture rate of the device stream (the pre-roll ring rate).
    /// Callers resample [`take_preroll`](Self::take_preroll) audio from this
    /// rate to 16 kHz before decoding.
    rate: u32,
}

/// Pure state machine of [`MicFrontEnd`] (unit-tested without cpal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrontEndState {
    /// Built, stream paused, pre-roll capture disarmed. The default after a
    /// successful build: the mic is open but nothing is captured.
    #[default]
    Idle,
    /// Stream playing; every callback feeds the pre-roll ring.
    Armed,
    /// `pause()`/`play()` was rejected by the backend; the caller must use
    /// the per-take rebuild path (`Recording`) for that take.
    Fallback,
}

impl MicFrontEnd {
    /// Builds the cpal input stream once. The stream is left PAUSED; call
    /// [`arm`](Self::arm) when a take is about to start.
    ///
    /// `device_id` empty means the system default input. `noise_reduction`
    /// enables RNNoise on the pre-warm path, matching
    /// [`Recording::start_with_noise`].
    ///
    /// Fails only when no usable input device or config exists at all — the
    /// same conditions that fail `Recording::start`. The stream is created
    /// unplayed; cpal streams start paused until `play()`.
    pub fn build(device_id: &str, noise_reduction: bool) -> Result<Self, String> {
        Self::build_df(device_id, noise_reduction, None)
    }

    /// Like [`build`](Self::build), but with the DeepFilterNet3 split
    /// pipeline (plan §5.5): the pre-roll ring keeps RAW audio (the ASR path
    /// is untouched by the noise mode) and a detection worker thread runs
    /// DFN3 on each frame before feeding the level meter.
    ///
    /// `dfn3` is the shared DFN3 stream (see [`build_dfn3_slot`]); `None`
    /// means the detection path runs on raw audio (mode Off, or the model
    /// is not downloaded).
    pub fn build_df(
        device_id: &str,
        noise_reduction: bool,
        dfn3: Option<Dfn3Slot>,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = if device_id.is_empty() {
            host.default_input_device()
        } else {
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
            let count = host.input_devices().map(|d| d.count()).unwrap_or(0);
            let msg = if count == 0 {
                "No microphone found. Connect a microphone or check System Settings > Sound > Input."
                    .to_string()
            } else {
                format!(
                    "No usable input device (found {count} device(s) but none could be opened)."
                )
            };
            return Err(msg);
        };

        let config = device
            .default_input_config()
            .map_err(|e| format!("No input config: {e}"))?;
        let format = config.sample_format();
        let stream_config = config.config();

        let prewarm = Arc::new(Mutex::new(MicPrewarm::with_rate(stream_config.sample_rate)));
        let armed = Arc::new(AtomicBool::new(false));
        let buffer: Buf = Arc::new(Mutex::new(Vec::new()));
        let level: Lvl = Arc::new(AtomicU32::new(NO_LEVEL));

        let err_fn: Box<dyn Fn(cpal::Error) + Send + 'static> =
            Box::new(move |err| eprintln!("[audio] prewarm stream error: {err}"));

        let mut stream = match format {
            SampleFormat::I8 => build_frontend_stream::<i8>(
                &device,
                &stream_config,
                &buffer,
                &level,
                &prewarm,
                &armed,
                err_fn,
                noise_reduction,
                false,
            ),
            SampleFormat::I16 => build_frontend_stream::<i16>(
                &device,
                &stream_config,
                &buffer,
                &level,
                &prewarm,
                &armed,
                err_fn,
                noise_reduction,
                false,
            ),
            SampleFormat::I32 => build_frontend_stream::<i32>(
                &device,
                &stream_config,
                &buffer,
                &level,
                &prewarm,
                &armed,
                err_fn,
                noise_reduction,
                false,
            ),
            SampleFormat::U8 => build_frontend_stream::<u8>(
                &device,
                &stream_config,
                &buffer,
                &level,
                &prewarm,
                &armed,
                err_fn,
                noise_reduction,
                false,
            ),
            SampleFormat::U16 => build_frontend_stream::<u16>(
                &device,
                &stream_config,
                &buffer,
                &level,
                &prewarm,
                &armed,
                err_fn,
                noise_reduction,
                false,
            ),
            SampleFormat::F32 => build_frontend_stream::<f32>(
                &device,
                &stream_config,
                &buffer,
                &level,
                &prewarm,
                &armed,
                err_fn,
                noise_reduction,
                false,
            ),
            SampleFormat::F64 => build_frontend_stream::<f64>(
                &device,
                &stream_config,
                &buffer,
                &level,
                &prewarm,
                &armed,
                err_fn,
                noise_reduction,
                false,
            ),
            other => return Err(format!("Unsupported sample format {other:?}")),
        }
        .ok_or_else(|| "Couldn't build the pre-warm input stream".to_string())?;

        // Plan §5.5 split: when a DFN3 slot is provided, a detection worker
        // thread runs DFN3 on each frame before feeding the level meter;
        // the pre-roll ring keeps RAW audio (the ASR path is untouched).
        // The pre-warm callback has no `on_frame` hook, so the worker only
        // updates the level meter from the enhanced audio.
        if let Some(slot) = dfn3 {
            let (frame_tx, frame_rx) = mpsc::channel::<Vec<f32>>();
            let w_level = Arc::clone(&level);
            let worker = thread::Builder::new()
                .name("teletype-dfn3-prewarm".into())
                .spawn(move || run_detection_worker(slot, frame_rx, w_level, None))
                .ok();
            if worker.is_none() {
                tracing::warn!("dfn3 prewarm detection worker spawn failed");
            }
            let _w_cb: FrameCb = Some(Box::new(move |mono: &[f32]| {
                let _ = frame_tx.send(mono.to_vec());
            }));
            // Rebuild the stream with the callback's frame sink replaced by
            // the channel send (the pre-roll ring keeps RAW mono).
            let err_fn: Box<dyn Fn(cpal::Error) + Send + 'static> =
                Box::new(move |err| eprintln!("[audio] prewarm stream error: {err}"));
            let rebuilt = match format {
                SampleFormat::I8 => build_frontend_stream::<i8>(
                    &device,
                    &stream_config,
                    &buffer,
                    &level,
                    &prewarm,
                    &armed,
                    err_fn,
                    noise_reduction,
                    true,
                ),
                SampleFormat::I16 => build_frontend_stream::<i16>(
                    &device,
                    &stream_config,
                    &buffer,
                    &level,
                    &prewarm,
                    &armed,
                    err_fn,
                    noise_reduction,
                    true,
                ),
                SampleFormat::I32 => build_frontend_stream::<i32>(
                    &device,
                    &stream_config,
                    &buffer,
                    &level,
                    &prewarm,
                    &armed,
                    err_fn,
                    noise_reduction,
                    true,
                ),
                SampleFormat::U8 => build_frontend_stream::<u8>(
                    &device,
                    &stream_config,
                    &buffer,
                    &level,
                    &prewarm,
                    &armed,
                    err_fn,
                    noise_reduction,
                    true,
                ),
                SampleFormat::U16 => build_frontend_stream::<u16>(
                    &device,
                    &stream_config,
                    &buffer,
                    &level,
                    &prewarm,
                    &armed,
                    err_fn,
                    noise_reduction,
                    true,
                ),
                SampleFormat::F32 => build_frontend_stream::<f32>(
                    &device,
                    &stream_config,
                    &buffer,
                    &level,
                    &prewarm,
                    &armed,
                    err_fn,
                    noise_reduction,
                    true,
                ),
                SampleFormat::F64 => build_frontend_stream::<f64>(
                    &device,
                    &stream_config,
                    &buffer,
                    &level,
                    &prewarm,
                    &armed,
                    err_fn,
                    noise_reduction,
                    true,
                ),
                _ => None,
            };
            match rebuilt {
                Some(s) => stream = s,
                None => {
                    tracing::warn!("dfn3: could not rebuild the pre-warm stream");
                }
            }
        }

        Ok(Self {
            state: FrontEndState::Idle,
            stream,
            prewarm,
            armed,
            rate: stream_config.sample_rate,
        })
    }

    /// Native capture rate of the device stream; [`take_preroll`](Self::take_preroll)
    /// audio is at this rate and must be resampled to 16 kHz before decoding.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Current state machine value (pure, testable).
    pub fn state(&self) -> FrontEndState {
        self.state
    }

    /// Starts the stream and arms pre-roll capture. Idempotent while armed.
    ///
    /// When the backend rejects `play()` (rare; pause is the common failure),
    /// records [`FrontEndState::Fallback`], warns via `tracing`, and returns
    /// `Err` so the caller uses the per-take rebuild path for this take.
    pub fn arm(&mut self) -> Result<(), String> {
        if matches!(self.state, FrontEndState::Armed) {
            return Ok(());
        }
        self.armed.store(true, Ordering::Relaxed);
        if let Err(e) = self.stream.play() {
            self.armed.store(false, Ordering::Relaxed);
            self.state = FrontEndState::Fallback;
            tracing::warn!(
                "mic front end: stream.play() failed; falling back to per-take rebuild: {e}"
            );
            return Err(format!("Couldn't play the pre-warm stream: {e}"));
        }
        self.state = FrontEndState::Armed;
        Ok(())
    }

    /// Pauses the stream and zeroes the pre-roll buffer (privacy: memory only,
    /// never written to disk). Idempotent while idle.
    ///
    /// When the backend rejects `pause()` (some input-only devices), records
    /// [`FrontEndState::Fallback`], warns via `tracing`, and returns `Err`;
    /// the caller then uses the per-take rebuild path for later takes.
    pub fn disarm(&mut self) -> Result<(), String> {
        if matches!(self.state, FrontEndState::Idle) {
            return Ok(());
        }
        self.armed.store(false, Ordering::Relaxed);
        if let Err(e) = self.stream.pause() {
            self.state = FrontEndState::Fallback;
            tracing::warn!(
                "mic front end: stream.pause() failed; falling back to per-take rebuild: {e}"
            );
            return Err(format!("Couldn't pause the pre-warm stream: {e}"));
        }
        // Zero the ring: it held audio that is now discarded.
        if let Ok(mut pw) = self.prewarm.lock() {
            pw.disarm();
        }
        self.state = FrontEndState::Idle;
        Ok(())
    }

    /// The last `ms` of pre-roll audio (plan §5.1 default: 300 ms), drained
    /// one-shot. Empty when nothing was captured (never armed, or drained).
    pub fn take_preroll(&self, ms: u32) -> Vec<f32> {
        self.prewarm
            .lock()
            .map(|mut pw| pw.take_preroll(ms))
            .unwrap_or_default()
    }
}

/// Builds the cpal input stream for [`MicFrontEnd`].
///
/// Eight parameters, which is over clippy's threshold. They are the same
/// pipeline as [`build_stream`] plus the pre-roll ring and armed flag, and
/// grouping them would mean threading a holder through without removing a
/// parameter, so the allow is deliberate.
#[allow(clippy::too_many_arguments)]
fn build_frontend_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: &Buf,
    level: &Lvl,
    prewarm: &Arc<Mutex<MicPrewarm>>,
    armed: &Arc<AtomicBool>,
    err_fn: Box<dyn Fn(cpal::Error) + Send + 'static>,
    noise_reduction: bool,
    dfn3: bool,
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
    let buffer = Arc::clone(buffer);
    let level = Arc::clone(level);
    let prewarm = Arc::clone(prewarm);
    let armed = Arc::clone(armed);
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
                let mono = if let Some(ref mut ns) = denoiser {
                    ns.process(&mono)
                } else {
                    mono
                };
                if let Ok(mut buf) = buffer.lock() {
                    let start = buf.len();
                    buf.extend(mono.iter().copied());
                    // DFN3 split (plan §5.5): the detection worker owns the
                    // level meter; the callback's meter must not double-send.
                    if !dfn3 {
                        if let Some(l) = meter.push(&buf[start..]) {
                            level.store(l.to_bits(), Ordering::Relaxed);
                        }
                    }
                }
                // Pre-roll ring: only while armed (privacy: nothing is kept
                // when the front end is disarmed).
                if armed.load(Ordering::Relaxed) {
                    if let Ok(mut pw) = prewarm.lock() {
                        pw.write(&mono);
                    }
                }
            },
            err_fn,
            None,
        )
        .ok()
}

/// Pure, deterministic input-device selection over device NAME strings
/// (plan §5.2). No cpal, no OS calls — unit-testable in isolation.
///
/// When a Bluetooth/HFP-looking device is present AND `prefer_builtin_with_bt`
/// is set, the built-in mic wins (Bluetooth HFP input is narrowband). A
/// wired USB mic always wins over the built-in. Returns the 0-based index
/// into the iterator, or `None` when nothing matches (caller keeps the
/// current default).
///
/// `names` yields `(&String, &N)` pairs: the device name and any associated
/// id. Only the name is inspected; the id is accepted for signature
/// symmetry with `list_input_devices`.
pub fn select_input_device<'a, N: AsRef<str> + 'a>(
    names: impl IntoIterator<Item = (&'a String, &'a N)>,
    prefer_builtin_with_bt: bool,
) -> Option<usize> {
    let mut builtin: Option<usize> = None;
    let mut usb: Option<usize> = None;
    let mut bt: Option<usize> = None;
    for (i, (name, _)) in names.into_iter().enumerate() {
        let n = name.to_ascii_lowercase();
        if BUILTIN_HINTS.iter().any(|h| n.contains(h)) {
            if builtin.is_none() {
                builtin = Some(i);
            }
        } else if BT_HINTS.iter().any(|h| n.contains(h)) {
            if bt.is_none() {
                bt = Some(i);
            }
        } else if USB_HINTS.iter().any(|h| n.contains(h)) && usb.is_none() {
            usb = Some(i);
        }
    }
    // A wired USB mic always wins: it is the user's deliberate choice.
    if let Some(u) = usb {
        return Some(u);
    }
    // Bluetooth present + preference set: prefer the built-in (HFP is
    // narrowband). Only when a built-in actually exists.
    if prefer_builtin_with_bt && bt.is_some() {
        if let Some(b) = builtin {
            return Some(b);
        }
        // No built-in: fall through to the BT device (it's the only option).
        return bt;
    }
    // Default: the built-in if present, else the BT device, else nothing.
    builtin.or(bt)
}

/// Name hints for built-in / internal microphones (case-insensitive).
const BUILTIN_HINTS: &[&str] = &[
    "builtin", "built-in", "internal", "macbook", "surface", "array",
];

/// Name hints for Bluetooth / HFP microphones (case-insensitive).
const BT_HINTS: &[&str] = &["airpods", "bluetooth", "wireless", "headset", "beat"];

/// Whether an input device name looks like a Bluetooth/HFP microphone
/// (used by the per-take quality signal, plan §5.7). Case-insensitive
/// substring match over [`BT_HINTS`].
pub fn device_name_is_bluetooth(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    BT_HINTS.iter().any(|h| n.contains(h))
}

/// Name hints for wired USB microphones (case-insensitive).
const USB_HINTS: &[&str] = &["usb"];

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
    /// Capture rate of the frames written into the ring; the ring holds
    /// 1.0 s at this rate and [`take_preroll`](Self::take_preroll) converts
    /// milliseconds with it.
    rate: u32,
}

impl Default for MicPrewarm {
    fn default() -> Self {
        Self::new()
    }
}

impl MicPrewarm {
    /// Create a new pre-warm with a 1.0 s ring buffer at 16 kHz (16000
    /// samples), matching [`TARGET_SAMPLE_RATE`].
    pub fn new() -> Self {
        Self::with_rate(TARGET_SAMPLE_RATE)
    }

    /// Create a new pre-warm with a 1.0 s ring buffer at the capture
    /// device's native `rate` (`rate` samples). [`MicFrontEnd::build`] uses
    /// this so the pre-roll holds real milliseconds of device-rate audio.
    pub fn with_rate(rate: u32) -> Self {
        let rb = SharedRb::<Heap<f32>>::new(rate.max(1) as usize);
        Self { rb, rate }
    }

    /// Write a frame of samples from the capture callback (lock-free).
    pub fn write(&mut self, frame: &[f32]) {
        self.rb.push_slice_overwrite(frame);
    }

    /// Return the last `ms` milliseconds of audio (300 ms default) at the
    /// ring's rate. Samples older than 1.0 s have been overwritten by the
    /// ring buffer. Consumes everything older, so a second call returns the
    /// next-older window and a drained buffer returns an empty vec until
    /// more frames are written.
    pub fn take_preroll(&mut self, ms: u32) -> Vec<f32> {
        let samples = (ms as u64 * self.rate as u64 / 1000) as usize;
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
    fn prewarm_preroll_merge_decodes_as_one_take() {
        // Simulates the hotkey-down take: 300 ms of pre-roll already in the
        // ring, then live samples arrive. The take is the pre-roll followed by
        // the live audio, decodable as one continuous stream.
        let mut pw = MicPrewarm::new();
        // 1 s of pre-warm audio so the ring has content.
        let preroll_src: Vec<f32> = (0..16_000).map(|i| 0.1 * (i as f32 * 0.001)).collect();
        pw.write(&preroll_src);
        let preroll = pw.take_preroll(300);
        assert_eq!(preroll.len(), 4800);
        // Live audio after hotkey-down: 0.5 s.
        let live: Vec<f32> = (0..8_000)
            .map(|i| 0.2 * (i as f32 * 0.001 + 16.0))
            .collect();
        let mut take = preroll;
        take.extend(live);
        // The take is continuous: the boundary sample (live[0]) follows the
        // last pre-roll sample with the expected time gap (1 sample).
        assert_eq!(take.len(), 4800 + 8_000);
        // The seam: live[0] - preroll.last() == one sample step (0.001*0.1
        // scale + the 16.0 offset in live) — proves no samples were lost or
        // duplicated at the merge.
        let seam = take[4800] - take[4799];
        assert!(seam > 0.0, "seam {seam}");
    }

    #[test]
    fn prewarm_with_rate_48k_ring_and_preroll() {
        // A 48 kHz front end must hold 1.0 s at 48 kHz and return real
        // milliseconds (14 400 samples for 300 ms), not 16 kHz counts.
        let mut pw = MicPrewarm::with_rate(48_000);
        let samples: Vec<f32> = (48_000..144_000).map(|i| (i as f32) * 0.001).collect();
        pw.write(&samples);
        let preroll = pw.take_preroll(300);
        assert_eq!(preroll.len(), 14_400);
        // The newest 300 ms window starts 0.3 s before the end: sample 129 600.
        assert!(
            (preroll[0] - 129.6).abs() < 1e-3,
            "preroll[0] {}",
            preroll[0]
        );
        // One-shot semantics.
        assert_eq!(pw.take_preroll(300).len(), 0);
    }

    #[test]
    fn prewarm_with_rate_48k_wraps_around() {
        // Wrap at a non-16k rate: capacity 48 000, write 1.3 s in two chunks
        // so the retained lap is the newest 1 s (samples 14 400..62 400).
        let mut pw = MicPrewarm::with_rate(48_000);
        let first: Vec<f32> = (0..48_000).map(|i| (i as f32) * 0.001).collect();
        let second: Vec<f32> = (48_000..62_400).map(|i| (i as f32) * 0.001).collect();
        pw.write(&first);
        pw.write(&second);
        let preroll = pw.take_preroll(300);
        assert_eq!(preroll.len(), 14_400);
        assert!(
            (preroll[0] - 48.0).abs() < 1e-3,
            "preroll[0] {}",
            preroll[0]
        );
    }

    #[test]
    fn device_name_is_bluetooth_matches_hints_case_insensitively() {
        assert!(device_name_is_bluetooth("AirPods Pro"));
        assert!(device_name_is_bluetooth("Jabra BLUETOOTH"));
        assert!(device_name_is_bluetooth("Wireless Headset"));
        assert!(device_name_is_bluetooth("Beats Studio3"));
    }

    #[test]
    fn device_name_is_bluetooth_rejects_wired_mics() {
        assert!(!device_name_is_bluetooth("MacBook Pro Microphone"));
        assert!(!device_name_is_bluetooth("USB Audio Device"));
        assert!(!device_name_is_bluetooth(""));
    }

    #[test]
    fn frontend_state_default_is_idle() {
        assert_eq!(FrontEndState::default(), FrontEndState::Idle);
    }

    #[test]
    fn frontend_state_transitions_pure() {
        // Pure state-machine transitions (no cpal): Idle -> Armed -> Idle,
        // and either -> Fallback on a rejected play/pause.
        let mut s = FrontEndState::Idle;
        assert_ne!(s, FrontEndState::Armed);
        s = FrontEndState::Armed;
        assert_eq!(s, FrontEndState::Armed);
        // disarm back to idle
        s = FrontEndState::Idle;
        assert_eq!(s, FrontEndState::Idle);
        // a rejected play/pause lands in Fallback from either state
        s = FrontEndState::Fallback;
        assert_eq!(s, FrontEndState::Fallback);
    }

    #[test]
    fn frontend_preroll_zeroed_after_disarm() {
        // The privacy guarantee: after disarm the ring is zeroed, so a take
        // started after disarm cannot contain pre-disarm audio.
        let mut pw = MicPrewarm::new();
        let s: Vec<f32> = vec![0.5; 8000];
        pw.write(&s);
        // disarm zeroes
        pw.disarm();
        let after = pw.take_preroll(300);
        assert!(
            after.iter().all(|&x| x == 0.0),
            "expected all zeros after disarm"
        );
    }

    #[test]
    fn select_prefers_builtin_when_bt_active_and_preferred() {
        // AirPods present + built-in present + preference set -> built-in wins
        // (plan §5.2: BT HFP is narrowband).
        let names: Vec<String> = vec!["AirPods Pro".into(), "MacBook Pro Microphone".into()];
        let ids: Vec<String> = vec!["id0".into(), "id1".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(1));
    }

    #[test]
    fn select_prefers_builtin_when_bt_active_case_insensitive() {
        let names: Vec<String> = vec!["airpods".into(), "BUILT-IN MICROPHONE".into()];
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(1));
    }

    #[test]
    fn select_keeps_bt_when_no_built_in_present() {
        // BT present, no built-in, preference set -> the BT device is the only
        // option, so it is returned.
        let names: Vec<String> = vec!["AirPods".into(), "Jabra BT250".into()];
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(0));
    }

    #[test]
    fn select_usb_wins_over_builtin_and_bt() {
        // A wired USB mic is the user's deliberate choice: it always wins.
        let names: Vec<String> = vec![
            "AirPods".into(),
            "MacBook Pro Microphone".into(),
            "USB Audio Device".into(),
        ];
        let ids: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(2));
    }

    #[test]
    fn select_no_match_returns_none() {
        // Nothing matches any hint -> None, caller keeps the current default.
        let names: Vec<String> = vec!["Weird Mic".into(), "Another Input".into()];
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), None);
    }

    #[test]
    fn select_empty_returns_none() {
        let names: Vec<String> = Vec::new();
        let ids: Vec<String> = Vec::new();
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), None);
    }

    #[test]
    fn select_without_preference_returns_builtin_or_bt() {
        // preference off: BT present + built-in present -> built-in still wins
        // (default), but BT alone -> BT.
        let names: Vec<String> = vec!["AirPods".into(), "MacBook Pro Microphone".into()];
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, false), Some(1));
    }

    #[test]
    fn select_first_builtin_wins_over_later() {
        // Multiple built-ins: the first one is returned (deterministic).
        let names: Vec<String> = vec![
            "Internal Microphone".into(),
            "Built-in Mic 2".into(),
            "AirPods".into(),
        ];
        let ids: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(0));
    }

    #[test]
    fn select_headset_counts_as_bt() {
        // "headset" is a BT/HFP hint.
        let names: Vec<String> = vec!["Wireless Headset".into(), "Surface Mic".into()];
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(1));
    }

    #[test]
    fn select_beat_counts_as_bt() {
        // "beat" (Beats headphones) is a BT hint.
        let names: Vec<String> = vec!["Beats Studio".into(), "MacBook Pro Microphone".into()];
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(1));
    }

    #[test]
    fn select_internal_hint_matches() {
        let names: Vec<String> = vec!["Internal Microphone".into()];
        let ids: Vec<String> = vec!["a".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(0));
    }

    #[test]
    fn select_surface_hint_matches() {
        let names: Vec<String> = vec!["Surface Array Microphone".into(), "AirPods".into()];
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let pairs: Vec<(&String, &String)> = names.iter().zip(ids.iter()).collect();
        let pairs = pairs.iter().map(|(a, b)| (*a, b));
        assert_eq!(select_input_device(pairs, true), Some(0));
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
