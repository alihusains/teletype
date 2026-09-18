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
}

/// A running microphone capture.
pub struct Recording {
    stop_tx: Sender<()>,
    done_rx: mpsc::Receiver<Captured>,
}

impl Recording {
    /// Starts recording from `device_id`, or the system default when empty.
    /// `on_level` receives smoothed loudness 0..1 on the capture thread.
    pub fn start(
        device_id: &str,
        on_level: impl Fn(f32) + Send + 'static,
    ) -> Result<Recording, String> {
        let device_id = device_id.to_string();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (done_tx, done_rx) = mpsc::channel::<Captured>();

        let started = thread::Builder::new()
            .name("teletype-audio".into())
            .spawn(move || {
                run_capture(&device_id, stop_rx, ready_tx, done_tx, on_level);
            });
        let started = started.map_err(|e| format!("Couldn't start audio: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Recording { stop_tx, done_rx }),
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
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

fn run_capture(
    device_id: &str,
    stop_rx: mpsc::Receiver<()>,
    ready_tx: Sender<Result<(), String>>,
    done_tx: Sender<Captured>,
    on_level: impl Fn(f32) + Send,
) {
    let host = cpal::default_host();
    let device = if device_id.is_empty() {
        host.default_input_device()
    } else {
        host.input_devices().ok().and_then(|devices| {
            devices
                .into_iter()
                .find(|d| d.id().ok().map(|id| id.to_string()).as_deref() == Some(device_id))
        })
    };
    let Some(device) = device else {
        let _ = ready_tx.send(Err("No input device available".into()));
        return;
    };

    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("No input config: {e}")));
            return;
        }
    };
    let sample_rate = config.sample_rate() as u32;
    let _channels = usize::from(config.channels()).max(1);
    let format = config.sample_format();
    let stream_config = config.config();

    let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(vec![
        0f32;
        (sample_rate as usize)
            * RESERVE_SECS
    ]));
    let level: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));

    let buf_clone = Arc::clone(&buffer);
    let level_clone = Arc::clone(&level);

    let err_fn = Box::new(|err| eprintln!("[audio] stream error: {err}"));
    let stream = match format {
        SampleFormat::I8 => {
            build_stream::<i8>(&device, &stream_config, buf_clone, level_clone, err_fn)
        }
        SampleFormat::I16 => {
            build_stream::<i16>(&device, &stream_config, buf_clone, level_clone, err_fn)
        }
        SampleFormat::I32 => {
            build_stream::<i32>(&device, &stream_config, buf_clone, level_clone, err_fn)
        }
        SampleFormat::U8 => {
            build_stream::<u8>(&device, &stream_config, buf_clone, level_clone, err_fn)
        }
        SampleFormat::U16 => {
            build_stream::<u16>(&device, &stream_config, buf_clone, level_clone, err_fn)
        }
        SampleFormat::F32 => {
            build_stream::<f32>(&device, &stream_config, buf_clone, level_clone, err_fn)
        }
        SampleFormat::F64 => {
            build_stream::<f64>(&device, &stream_config, buf_clone, level_clone, err_fn)
        }
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
    let _ = ready_tx.send(Ok(()));

    let last_level = Instant::now();
    loop {
        match stop_rx.recv_timeout(LEVEL_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if last_level.elapsed() >= LEVEL_INTERVAL {
                    let bits = level.load(Ordering::Relaxed);
                    let level_val = f32::from_bits(bits).min(1.0);
                    on_level(level_val);
                }
            }
        }
    }

    let raw = buffer.lock().map(|b| b.clone()).unwrap_or_default();
    let resampled = resample_to_target(&raw, sample_rate);
    let duration_secs = resampled.len() as f64 / TARGET_SAMPLE_RATE as f64;
    let _ = done_tx.send(Captured {
        samples: resampled,
        duration_secs,
    });
}

type Buf = Arc<Mutex<Vec<f32>>>;
type Lvl = Arc<AtomicU32>;

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: Buf,
    level: Lvl,
    err_fn: Box<dyn Fn(cpal::Error) + Send + 'static>,
) -> Option<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels).max(1);
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
                    buf.extend(mono);
                    // Compute a simple RMS level over the last chunk.
                    let recent = &buf[start.min(buf.len().saturating_sub(4096))..];
                    if !recent.is_empty() {
                        let rms = recent.iter().map(|s| s * s).sum::<f32>()
                            / (recent.len() as f32).sqrt();
                        level.store(rms.min(1.0).to_bits(), Ordering::Relaxed);
                    }
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
