//! P3.17: decode + resample seam for file transcription.
//!
//! The full `transcribe_file` command needs the speech model loaded, so this
//! test exercises the decode helper directly: a 3-second 16 kHz mono WAV of a
//! 440 Hz tone must decode to ~3 s of 16 kHz PCM through symphonia.

const TARGET_RATE: u32 = 16_000;

fn make_wav(path: &std::path::Path, secs: u32, rate: u32, hz: f64) {
    use std::io::Write;
    let n = (secs * rate) as usize;
    let mut data = Vec::with_capacity(n * 2);
    for i in 0..n {
        let v = (8000.0 * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin()) as i16;
        data.extend_from_slice(&v.to_le_bytes());
    }
    let mut w = std::fs::File::create(path).expect("create wav");
    // RIFF header
    let total = 36 + data.len() as u32;
    w.write_all(b"RIFF").unwrap();
    w.write_all(&total.to_le_bytes()).unwrap();
    w.write_all(b"WAVE").unwrap();
    w.write_all(b"fmt ").unwrap();
    w.write_all(&16u32.to_le_bytes()).unwrap(); // fmt chunk size
    w.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    w.write_all(&1u16.to_le_bytes()).unwrap(); // mono
    w.write_all(&rate.to_le_bytes()).unwrap();
    w.write_all(&(rate * 2).to_le_bytes()).unwrap(); // byte rate
    w.write_all(&2u16.to_le_bytes()).unwrap(); // block align
    w.write_all(&16u16.to_le_bytes()).unwrap(); // bits per sample
    w.write_all(b"data").unwrap();
    w.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
    w.write_all(&data).unwrap();
}

#[test]
fn decodes_wav_to_16khz_mono() {
    let dir = std::env::temp_dir().join("teletype_p317_test");
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tt_test.wav");
    make_wav(&wav, 3, 16_000, 440.0);

    let pcm = teletype_desktop_lib::decode_audio_file_public(&wav).expect("decode");

    // ~3 s at 16 kHz, within a small margin for resampler rounding.
    let secs = pcm.len() as f64 / TARGET_RATE as f64;
    assert!(
        (2.8..=3.2).contains(&secs),
        "expected ~3 s of audio, got {secs} s"
    );
    // The tone should not be silent.
    let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.1, "tone too quiet: peak {peak}");
}

#[test]
fn resamples_44khz_wav_to_16khz() {
    let dir = std::env::temp_dir().join("teletype_p317_test");
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tt_test_44k.wav");
    make_wav(&wav, 3, 44_100, 440.0);

    let pcm = teletype_desktop_lib::decode_audio_file_public(&wav).expect("decode");

    let secs = pcm.len() as f64 / TARGET_RATE as f64;
    assert!(
        (2.8..=3.2).contains(&secs),
        "expected ~3 s after 44.1k->16k resample, got {secs} s"
    );
}
