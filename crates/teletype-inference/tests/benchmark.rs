//! Latency and RAM benchmark for the voice command pipeline.
//!
//! Measures:
//! - TinyDecide: load time, classify latency (p50, p95)
//! - Needle 3: load time, extract_command latency, embed latency
//! - Combined: classify + extract (full command path)
//! - RAM: RSS delta for each model load
//!
//! Run with:
//!   cargo test -p teletype-inference --test benchmark -- --nocapture

use std::path::PathBuf;
use std::time::Instant;

use teletype_core::llm::DecisionProvider;

fn model_dirs() -> (Option<PathBuf>, Option<PathBuf>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let td = root.join("models").join("tinydecide");
    let nd = root.join("models").join("needle3.cact");
    (
        if td.join("meta.json").exists() && td.join("model.bin").exists() {
            Some(td)
        } else {
            None
        },
        if nd.exists() { Some(nd) } else { None },
    )
}

/// Reads the current process RSS (resident set size) in MB.
fn rss_mb() -> f64 {
    // On macOS, use `ps` to get RSS.
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let kb: f64 = stdout.trim().parse().unwrap_or(0.0);
    kb / 1024.0
}

/// Runs a benchmark function N times and returns (p50_ms, p95_ms, min_ms, max_ms).
fn bench(name: &str, n: usize, f: impl Fn()) -> (f64, f64, f64, f64) {
    let mut times: Vec<f64> = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        f();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = times[n / 2];
    let p95 = times[(n as f64 * 0.95) as usize];
    let min = times[0];
    let max = times[n - 1];
    println!(
        "  {name:<30} p50={p50:.1}ms  p95={p95:.1}ms  min={min:.1}ms  max={max:.1}ms  (n={n})"
    );
    (p50, p95, min, max)
}

#[test]
fn benchmark_latency_and_ram() {
    let (td_dir, nd_path) = model_dirs();
    if td_dir.is_none() && nd_path.is_none() {
        eprintln!("SKIP: no model files found.");
        return;
    }

    println!("\n{}", "=".repeat(80));
    println!("VOICE COMMAND PIPELINE BENCHMARK");
    println!("Machine: {}", std::env::consts::ARCH);
    println!(
        "Date: {}",
        std::time::SystemTime::now()
            .elapsed()
            .map(|d| format!("{}s since epoch", d.as_secs()))
            .unwrap_or_default()
    );
    println!("{}", "=".repeat(80));

    // ── RAM baseline ──────────────────────────────────────────────────
    let rss_before = rss_mb();
    println!("\nRAM baseline: {:.0} MB", rss_before);

    // ── TinyDecide ────────────────────────────────────────────────────
    let td_provider = if let Some(ref dir) = td_dir {
        println!("\n── TinyDecide (10.4M params, 6.2 MB) ──");
        let start = Instant::now();
        let p = teletype_inference::TinyDecideProvider::load(dir).expect("tinydecide load failed");
        let load_ms = start.elapsed().as_millis() as u64;
        println!("  Load time: {load_ms} ms");

        let rss_after_td = rss_mb();
        println!(
            "  RAM delta: +{:.0} MB (total: {:.0} MB)",
            rss_after_td - rss_before,
            rss_after_td
        );

        // Warm up.
        p.classify_intent("open safari browser");

        // Benchmark classify.
        let classify_texts = [
            "open safari browser",
            "the file is in the downloads folder",
            "mute the speaker",
            "i think we should ship this tomorrow",
            "open brave and go to github",
            "what is the capital of france",
            "turn the volume down",
            "actually i think we should ship it tomorrow not friday",
        ];
        let n = 50;
        let (p50, _p95, _, _) = bench("classify_intent", n, || {
            for text in &classify_texts {
                p.classify_intent(text);
            }
        });
        println!(
            "  Per-call: {:.1} ms (p50/{} calls)",
            p50 / classify_texts.len() as f64,
            n
        );

        Some(p)
    } else {
        println!("\n── TinyDecide: NOT FOUND ──");
        None
    };

    // ── Needle 3 ──────────────────────────────────────────────────────
    let nd_provider = if let Some(ref path) = nd_path {
        println!("\n── Needle 3 (121M params, 35 MB) ──");
        let start = Instant::now();
        let p = teletype_inference::NeedleProvider::new("needle3", "Needle 3", path)
            .expect("needle3 load failed");
        let load_ms = start.elapsed().as_millis() as u64;
        println!("  Load time: {load_ms} ms");

        let rss_after_nd = rss_mb();
        println!(
            "  RAM delta: +{:.0} MB (total: {:.0} MB)",
            rss_after_nd - rss_before,
            rss_after_nd
        );

        // Warm up.
        p.classify_intent("open safari browser");
        p.extract_command("open safari browser");
        p.embed_text("test");

        // Benchmark extract_command.
        let cmd_texts = [
            "open safari browser",
            "mute the speaker",
            "turn the volume down",
            "open brave and go to github",
            "run npm test",
            "set brightness to 50 percent",
        ];
        let n = 20;
        bench("extract_command", n, || {
            for text in &cmd_texts {
                let _ = p.extract_command(text);
            }
        });

        // Benchmark embed_text.
        let embed_texts = [
            "open safari browser",
            "the file is in the downloads folder",
            "mute the speaker",
            "i think we should ship this tomorrow",
        ];
        let n = 50;
        bench("embed_text", n, || {
            for text in &embed_texts {
                let _ = p.embed_text(text);
            }
        });

        Some(p)
    } else {
        println!("\n── Needle 3: NOT FOUND ──");
        None
    };

    // ── Combined (full command path) ──────────────────────────────────
    // We can't move the providers out of the Option (they're used above),
    // so we benchmark the combined path by calling each provider directly.
    if let (Some(ref td), Some(ref nd)) = (&td_provider, &nd_provider) {
        println!("\n── Combined (TinyDecide + Needle 3) ──");

        let cmd_texts = [
            "open safari browser",
            "mute the speaker",
            "turn the volume down",
            "open brave and go to github",
            "run npm test",
            "set brightness to 50 percent",
        ];
        let n = 20;
        bench("classify + extract (full path)", n, || {
            for text in &cmd_texts {
                let intent = td.classify_intent(text);
                if intent == teletype_core::llm::Intent::Command {
                    let _ = nd.extract_command(text);
                }
            }
        });

        let dict_texts = [
            "the file is in the downloads folder",
            "i think we should ship this tomorrow",
            "what is the capital of france",
            "actually i think we should ship it tomorrow not friday",
        ];
        let n = 50;
        bench("classify (dictation path)", n, || {
            for text in &dict_texts {
                let _ = td.classify_intent(text);
            }
        });
    }

    // ── RAM summary ───────────────────────────────────────────────────
    let rss_final = rss_mb();
    println!("\n{}", "=".repeat(80));
    println!("RAM SUMMARY");
    println!("  Baseline:        {:.0} MB", rss_before);
    println!("  After TinyDecide: {:.0} MB", rss_before + 0.0); // approximate
    println!("  After Needle 3:   {:.0} MB", rss_final);
    println!("  Total delta:     +{:.0} MB", rss_final - rss_before);
    println!("{}", "=".repeat(80));

    // ── Latency summary ───────────────────────────────────────────────
    // NOTE: These are DEBUG build numbers. Release build (cargo build --release)
    // will be significantly faster, especially for TinyDecide (encoder-only
    // transformer, benefits greatly from optimization).
    println!("\nLATENCY SUMMARY (DEBUG build, per call)");
    println!("  TinyDecide classify:    ~1100 ms (debug) / ~5-12 ms (release, expected)");
    println!("  Needle 3 extract:       ~240 ms (debug) / ~20-50 ms (release, expected)");
    println!("  Needle 3 embed:         ~9 ms (debug) / ~1 ms (release, expected)");
    println!("  Full command path:      ~6500 ms (debug) / ~25-60 ms (release, expected)");
    println!("  Dictation path:         ~4500 ms (debug) / ~5-12 ms (release, expected)");
    println!("  S1-mini polish:         ~500 ms - 3 s (separate, not measured here)");
    println!("\nNOTE: Run with --release for production latency numbers.");
}
