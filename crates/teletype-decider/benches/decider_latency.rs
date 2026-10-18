//! A.4 latency floor: `decider_ms` for a single `generate` call.
//!
//! Measures the full inference loop (audio encode + tool encode + greedy
//! decode + parse) on a 2-second silence clip with the v1 tool list. The
//! model is loaded once; the bench iterates `generate` N times and reports
//! mean / p50 / p95 ms.
//!
//! Skipped (exit 0) when the model dir is absent — same convention as the
//! integration tests in `tests/a4_no_mic.rs`.
//!
//! Run with:
//!   cargo bench -p teletype-decider --features onnx --bench decider_latency

use std::path::{Path, PathBuf};
use std::time::Instant;

use teletype_decider::{tools_to_json, v1_tools, NeuDecide};

fn model_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("NEUDECIDE_MODEL_DIR") {
        let p = PathBuf::from(d);
        return p.join("config.json").is_file().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = Path::new(&home).join(
        ".cache/huggingface/hub/models--neuphonic--neudecide/snapshots/fb0157e06da16d887caf041ca7929f96da419e6b",
    );
    p.join("config.json").is_file().then_some(p)
}

fn v1_tools_json() -> String {
    let apps = ["terminal", "safari", "notes", "mail", "finder", "code"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let snippets = ["thanks", "regards", "br", "signature"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let styles = ["body", "heading", "code"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    tools_to_json(&v1_tools(&apps, &snippets, &styles))
}

fn main() {
    let Some(dir) = model_dir() else {
        eprintln!("SKIP: model dir not found (set NEUDECIDE_MODEL_DIR)");
        return;
    };

    let mut model = NeuDecide::load(&dir).expect("load model");
    let tools_json = v1_tools_json();
    // 2 seconds of silence at 16 kHz.
    let pcm = vec![0.0f32; 32_000];

    // Warm up (first call includes session warmup).
    model.generate(&pcm, &tools_json).expect("warmup");

    const ITERS: usize = 60;
    let mut times_ms: Vec<f64> = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let t0 = Instant::now();
        model.generate(&pcm, &tools_json).expect("generate");
        times_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = times_ms.iter().sum::<f64>() / times_ms.len() as f64;
    let p50 = times_ms[times_ms.len() / 2];
    let p95 = times_ms[(times_ms.len() as f64 * 0.95) as usize];

    println!("decider latency (2 s silence, v1 tools, {ITERS} iters):");
    println!("  mean: {mean:.1} ms");
    println!("  p50:  {p50:.1} ms");
    println!("  p95:  {p95:.1} ms");
    println!("  min:  {:.1} ms", times_ms[0]);
    println!("  max:  {:.1} ms", times_ms[times_ms.len() - 1]);

    // A.4 pass criteria: p95 <= 80 ms on M1 (aarch64), <= 150 ms on 4-core x64.
    let budget: f64 = if cfg!(target_arch = "aarch64") {
        80.0
    } else {
        150.0
    };
    if p95 > budget {
        eprintln!("FAIL: p95 {p95:.1} ms exceeds {budget:.0} ms budget");
        std::process::exit(1);
    }
    println!("PASS: p95 within budget");
}
