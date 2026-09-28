//! Streaming-ASR feasibility spike (temporary; delete once the design is chosen).
//!
//! Two questions this answers, both of which decide the sliding-window design:
//!
//! 1. **Is a re-decode affordable?** Parakeet has no online decoder in this
//!    binding, so "streaming" means re-decoding a sliding window of captured
//!    audio on a cadence. Viable only if a re-decode is cheap versus the cadence.
//! 2. **How much does the hypothesis churn?** Because every decode starts from
//!    scratch, words near the end of the window get revised. The number of
//!    trailing seconds that must stay provisional is what the commit boundary
//!    is set from, so measure it rather than guess it.
//!
//! Usage:
//!   say -o /tmp/spike.wav --data-format=LEI16@16000 "long text ..."
//!   cargo run -p teletype-speech --example stream_spike -- /tmp/spike.wav <model>

#[cfg(target_os = "macos")]
mod mac {
    use std::path::Path;
    use std::time::Instant;

    use teletype_speech::{ParakeetProvider, SpeechProvider};

    const TARGET_RATE: u32 = 16_000;
    const REPEATS: usize = 5;

    /// Minimal 16-bit mono PCM WAV reader, so the spike needs no decode
    /// dependency in the speech crate.
    fn read_wav_mono16(path: &Path) -> Result<(Vec<f32>, u32), String> {
        let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("not a RIFF/WAVE file".into());
        }
        let mut pos = 12usize;
        let (mut rate, mut channels) = (0u32, 0u16);
        let mut samples: Option<Vec<f32>> = None;
        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let body = pos + 8;
            if id == b"fmt " && body + 16 <= bytes.len() {
                channels = u16::from_le_bytes(bytes[body + 2..body + 4].try_into().unwrap());
                rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().unwrap());
            } else if id == b"data" && body + size <= bytes.len() {
                let raw = &bytes[body..body + size];
                let frames = raw.len() / 2;
                let mut mono = Vec::with_capacity(frames);
                for i in 0..frames {
                    mono.push(i16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]) as f32 / 32768.0);
                }
                samples = Some(mono);
            }
            pos = body + size + (size & 1);
        }
        let samples = samples.ok_or("no data chunk")?;
        if channels > 1 {
            let ch = channels as usize;
            let frames = samples.len() / ch;
            let mut mixed = Vec::with_capacity(frames);
            for f in 0..frames {
                let sum: f32 = samples[f * ch..f * ch + ch].iter().sum();
                mixed.push(sum / ch as f32);
            }
            return Ok((mixed, rate));
        }
        Ok((samples, rate))
    }

    /// Nearest-rank percentile on a sorted slice.
    fn pct(sorted: &[f64], p: f64) -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
        sorted[rank - 1]
    }

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                    .to_lowercase()
            })
            .filter(|w| !w.is_empty())
            .collect()
    }

    /// Words of `window` that are still agreed with by `full`, counting from the
    /// END of `window` backwards while they match.
    ///
    /// This is the metric that matters for a streaming decoder. The newest
    /// hypothesis covers the trailing audio, so its words align with the *tail*
    /// of the authoritative full decode, not its head. Measuring a shared
    /// prefix against the full decode is meaningless here, and measuring a
    /// shared suffix between two consecutive windows is meaningless too,
    /// because consecutive windows are offset in time: the older window's tail
    /// is the newer window's *head*.
    fn agreed_tail(window: &[String], full: &[String]) -> usize {
        let mut n = 0;
        while n < window.len() && n < full.len() {
            let w = &window[window.len() - 1 - n];
            let f = &full[full.len() - 1 - n];
            if w != f {
                break;
            }
            n += 1;
        }
        n
    }

    /// Re-decode the trailing window at each step, returning `(step_index, text)`.
    /// Used to dump raw hypotheses for eyeballing before trusting any derived
    /// stability metric.
    fn series_from_probe(
        p: &mut ParakeetProvider,
        audio: &[f32],
        w: usize,
        step: usize,
        rate: u32,
    ) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        let mut end = w;
        while end <= audio.len() {
            let t_s = end / rate as usize;
            if let Ok(hyp) = p.transcribe(&audio[end - w..end], "en") {
                out.push((t_s, hyp));
            }
            end += step;
        }
        out
    }

    pub fn run(wav: &Path, model: &Path) -> Result<(), String> {
        let (audio, rate) = read_wav_mono16(wav)?;
        if rate != TARGET_RATE {
            eprintln!("note: wav is {rate} Hz, providers expect {TARGET_RATE} Hz");
        }
        let total_s = audio.len() as f64 / rate as f64;
        println!("audio: {} samples @ {rate} Hz = {total_s:.2}s", audio.len());

        let mut p = ParakeetProvider::new();
        let t0 = Instant::now();
        p.load(model).map_err(|e| e.to_string())?;
        println!("model load: {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);

        let t = Instant::now();
        let first = p.transcribe(&audio, "en").map_err(|e| e.to_string())?;
        println!(
            "first full decode ({total_s:.2}s): {:.0} ms -> {:?}",
            t.elapsed().as_secs_f64() * 1000.0,
            first.chars().take(60).collect::<String>()
        );
        let t = Instant::now();
        p.transcribe(&audio, "en").map_err(|e| e.to_string())?;
        println!(
            "warm full decode:        {:.0} ms",
            t.elapsed().as_secs_f64() * 1000.0
        );

        // ---- Q1: cost of one decode, by window length ----
        println!("\n== Q1: cost of one decode, by window length ({REPEATS} runs each) ==");
        println!(
            "{:>8}  {:>9}  {:>9}  {:>8}",
            "window", "p50 ms", "p95 ms", "xRT"
        );
        for secs in [2.0f64, 4.0, 6.0, 8.0, 11.0, 16.0] {
            let n = ((secs * rate as f64) as usize).min(audio.len());
            if n < rate as usize {
                continue;
            }
            let window = &audio[..n];
            let mut ms = Vec::new();
            for _ in 0..REPEATS {
                let t = Instant::now();
                let _ = p.transcribe(window, "en");
                ms.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{secs:>7.0}s  {:>9.0}  {:>9.0}  {:>7.3}x",
                pct(&ms, 0.5),
                pct(&ms, 0.95),
                pct(&ms, 0.5) / 1000.0 / secs
            );
        }

        // ---- Q1b: sustained cost at each cadence, over the whole utterance ----
        println!("\n== Q1b: sustained sliding window over the full {total_s:.2}s utterance ==");
        println!(
            "{:>8}  {:>8}  {:>9}  {:>9}  {:>10}  {:>8}",
            "window", "cadence", "hyp p50", "hyp p95", "cpu/audio", "decodes"
        );
        for (window_s, cadence_s) in [
            (8.0f64, 1.0f64),
            (11.0, 0.5),
            (11.0, 1.0),
            (11.0, 2.0),
            (16.0, 1.0),
            (16.0, 2.0),
        ] {
            let w = ((window_s * rate as f64) as usize).min(audio.len());
            let step = (cadence_s * rate as f64) as usize;
            let mut ms = Vec::new();
            let mut cpu = 0.0f64;
            let mut end = w;
            while end <= audio.len() {
                let t = Instant::now();
                let _ = p.transcribe(&audio[end - w..end], "en");
                let d = t.elapsed().as_secs_f64() * 1000.0;
                ms.push(d);
                cpu += d;
                end += step;
            }
            if ms.len() < 2 {
                continue;
            }
            ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{window_s:>7.0}s  {cadence_s:>7.1}s  {:>9.0}  {:>9.0}  {:>9.3}x  {:>8}",
                pct(&ms, 0.5),
                pct(&ms, 0.95),
                (cpu / 1000.0) / total_s,
                ms.len()
            );
        }

        // ---- Q2: hypothesis churn, which sets the commit boundary ----
        // Replay the utterance as if it were arriving live. Window T covers
        // [T-window, T]; window T+step covers [T-window+step, T+step]. The two
        // therefore share `window - step` seconds of audio, so the NEW
        // hypothesis should begin with the words the previous one ended with,
        // minus whatever falls inside the newly added `step` seconds.
        let window_s = 11.0f64;
        let step_s = 1.0f64;
        let w = ((window_s * rate as f64) as usize).min(audio.len());
        let step = (step_s * rate as f64) as usize;
        let final_text = p.transcribe(&audio, "en").map_err(|e| e.to_string())?;
        let final_words = words(&final_text);

        println!("\n== Q2a: overlap stability (window {window_s:.0}s, step {step_s:.0}s) ==");
        // Dump raw hypotheses first: the derived numbers are only trustworthy
        // if the underlying text has been eyeballed.
        for (i, hyp) in series_from_probe(&mut p, &audio, w, step, rate)
            .into_iter()
            .take(3)
        {
            println!("  [t={i:>2}s] {hyp}");
        }
        println!(
            "{:>6}  {:>7}  {:>13}  {:>10}  {:>12}",
            "t", "words", "agreed tail", "unstable", "unstable secs"
        );
        // Per-step word lists, for the commit simulation below.
        let mut series: Vec<Vec<String>> = Vec::new();
        let mut agreed_all: Vec<usize> = Vec::new();
        let words_per_s = final_words.len() as f64 / total_s;
        let mut end = w;
        while end <= audio.len() {
            let t_s = end as f64 / rate as f64;
            let hyp = p
                .transcribe(&audio[end - w..end], "en")
                .map_err(|e| e.to_string())?;
            let cur = words(&hyp);
            let agreed = agreed_tail(&cur, &final_words);
            let unstable = cur.len().saturating_sub(agreed);
            agreed_all.push(agreed);
            println!(
                "{t_s:>5.0}s  {:>7}  {:>13}  {:>10}  {:>11.1}s",
                cur.len(),
                agreed,
                unstable,
                unstable as f64 / words_per_s.max(0.01)
            );
            series.push(cur);
            end += step;
        }

        if series.is_empty() {
            println!("(not enough audio for a churn measurement; use a longer clip)");
            return Ok(());
        }
        let mut sorted = agreed_all.clone();
        sorted.sort_unstable();
        println!(
            "\nwords at the end of each hypothesis that the final decode confirms: \
             min {}, p50 {}, max {} (of ~{} words)",
            sorted[0],
            sorted[sorted.len() / 2],
            sorted[sorted.len() - 1],
            final_words.len()
        );

        // ---- Q3: which words are safe to show as FINAL? ----
        // A streaming pill shows a "committed" line and a "still changing"
        // tail. A word is safe to commit once it is far enough from the live
        // edge that Parakeet stops revising it. `unstable secs` above is that
        // distance for this clip.
        //
        // The practical consequence for the design: commit everything except the
        // last N seconds of hypothesis, where N is derived from the measured
        // churn, not guessed. Re-decoding the window from the committed audio
        // on every step is what makes the tail unstable.
        let mut unstable_secs: Vec<f64> = Vec::new();
        let mut end2 = w;
        while end2 <= audio.len() {
            let hyp = p
                .transcribe(&audio[end2 - w..end2], "en")
                .map_err(|e| e.to_string())?;
            let cur = words(&hyp);
            let unstable = cur.len().saturating_sub(agreed_tail(&cur, &final_words));
            unstable_secs.push(unstable as f64 / words_per_s.max(0.01));
            end2 += step;
        }
        unstable_secs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("\n== Q3: provisional tail size ==");
        println!(
            "unstable tail: p50 {:.1}s, p90 {:.1}s, max {:.1}s of audio",
            unstable_secs[unstable_secs.len() / 2],
            unstable_secs[((0.90 * unstable_secs.len() as f64).ceil() as usize)
                .clamp(1, unstable_secs.len())
                - 1],
            unstable_secs[unstable_secs.len() - 1]
        );
        println!(
            "=> a pill that commits all but the last {:.0}s of hypothesis will show \
             stable text, at the cost of that much latency on the final words.",
            unstable_secs[((0.90 * unstable_secs.len() as f64).ceil() as usize)
                .clamp(1, unstable_secs.len())
                - 1]
        );

        println!(
            "\nfinal decode: {} words over {total_s:.2}s = {words_per_s:.2} words/s",
            final_words.len()
        );
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("stream_spike is macOS-only (parakeet is a macOS engine)");
}

#[cfg(target_os = "macos")]
fn main() {
    use std::path::Path;
    let mut args = std::env::args().skip(1);
    let wav = args.next().expect("usage: stream_spike <wav> <model>");
    let model = args.next().expect("usage: stream_spike <wav> <model>");
    if let Err(e) = mac::run(Path::new(&wav), Path::new(&model)) {
        eprintln!("spike failed: {e}");
        std::process::exit(1);
    }
}
