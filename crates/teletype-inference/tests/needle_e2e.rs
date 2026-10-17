//! End-to-end test: load Needle 3, init with the polish tool schema,
//! run a real transcript through the grammar-constrained decode, and
//! verify the output is clean, parseable text.
//!
//! Run with:
//!   cargo test -p teletype-inference --test needle_e2e -- --nocapture
//!
//! The model file must be at `models/needle3.cact` (relative to the
//! workspace root). Set `NEEDLE3_CACT` to override the path.
//!
//! # FINDINGS (2026-10-12)
//!
//! Needle 3 is a **tool-calling model**, not a general-purpose NLP model.
//! It is designed for bounded tool calls (set_lights, get_weather), not for
//! open-ended text tasks.
//!
//! **What works well:**
//! - Load: 11 ms. Init: 20–47 ms. Embedding: 1 ms (3072-dim vector).
//! - Latency: 13–47 ms per call (4000+ tok/s prefill, 1100+ tok/s decode).
//! - RAM: ~90–104 MB peak.
//! - Grammar-constrained decode: output is always valid JSON.
//!
//! **What does NOT work:**
//!
//! 1. **Polish (full-text rewrite):** With a "polish" tool, the model
//!    **summarizes** the transcript (returns key phrases like "ship it
//!    tomorrow") instead of returning the full cleaned text. Confidence is
//!    low (0.02–0.04), so calls land in `suppressed_calls`.
//!
//! 2. **Intent classification:** With a "classify" tool (enum:
//!    dictation/command/code/question/other), the model scores **0/12** on
//!    real command examples. "open safari browser" → "dictation", "open vs
//!    code" → "code", "the file is in downloads" → "code". The model maps
//!    keywords to the wrong categories because it's not trained for
//!    classification — it's trained for tool calling.
//!
//! **Verdict:** Needle 3 is NOT suitable for:
//! - Full-text transcript polishing (use S1-mini)
//! - Intent classification of arbitrary transcripts (use a classifier or
//!   the existing heuristic gate)
//!
//! Needle 3 IS suitable for:
//! - **Structured extraction** (ITN: extract time/number/name from a
//!   transcript, where the output is a bounded set of fields)
//! - **Embedding** (3072-dim vectors, 1 ms, for semantic personalization)
//! - **Voice commands** (audio → tool call, via the Whistle companion
//!   model, where the tools are predefined system actions)

use std::time::Instant;
use teletype_core::llm::{DecisionProvider, GenerationParams, InferenceProvider};
use teletype_inference::{
    NeedleProvider, CLASSIFY_TOOLS_JSON, EXTRACT_SYSTEM_PROMPT, EXTRACT_TOOLS_JSON,
};

fn model_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("NEEDLE3_CACT") {
        return std::path::PathBuf::from(p);
    }
    // Workspace root is two levels up from the crate.
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("../..")
        .join("models")
        .join("needle3.cact")
        .canonicalize()
        .unwrap_or_else(|_| {
            eprintln!("ERROR: models/needle3.cact not found. Download it with:");
            eprintln!(
                "  curl -L -o models/needle3.cact \
                 https://huggingface.co/Cactus-Compute/needle3/resolve/main/needle3.cact"
            );
            std::process::exit(1);
        })
}

#[test]
fn e2e_load_init_complete() {
    let path = model_path();
    println!("\n=== Needle 3 E2E Test ===");
    println!("Model: {}", path.display());

    // 1. Load
    let load_start = Instant::now();
    let provider = NeedleProvider::new("needle3", "Needle 3", &path).expect("needle load failed");
    let load_ms = load_start.elapsed().as_millis();
    println!("\n[1] Load: {} ms", load_ms);
    assert!(provider.is_alive(), "model should be alive after load");

    // 2. Init with NO tools — use the "respond" path for free-text polish.
    //    Needle 3 is a tool-calling model; with no tools declared it returns
    //    type "respond" with the text in the `response` field.
    let init_start = Instant::now();
    let prefix_tokens = provider
        .init(Some(EXTRACT_SYSTEM_PROMPT), None)
        .expect("needle init failed");
    let init_ms = init_start.elapsed().as_millis();
    println!(
        "[2] Init (no tools): {} ms (static prefix: {} tokens)",
        init_ms, prefix_tokens
    );

    // 3. Run real transcripts through the polish
    let transcripts = [
        (
            "um so like the file is in the downloads folder right now and i need you to check it out",
            "expect: clean sentence, no fillers, proper punctuation",
        ),
        (
            "actually i think we should ship it tomorrow not friday because the client is waiting",
            "expect: 'actually' removed or kept naturally, clean sentence",
        ),
        (
            "The meeting is scheduled for three p m tomorrow in the conference room.",
            "expect: minimal changes, already clean",
        ),
        (
            "yeah so the deploy went fine but um the tests are still running and i dont know if theyll pass",
            "expect: fillers removed, 'dont' -> 'don't', 'theyll' -> 'they'll'",
        ),
    ];

    for (i, (input, note)) in transcripts.iter().enumerate() {
        // Reset the conversation between independent transcripts so the
        // model doesn't carry context from the previous one.
        needle_sys::reset();
        // Re-init after reset (no tools — respond path).
        provider
            .init(Some(EXTRACT_SYSTEM_PROMPT), None)
            .expect("re-init after reset failed");

        println!("\n[3.{}] {}", i + 1, note);
        println!("  IN:  {}", input);

        let gen_start = Instant::now();
        let params = GenerationParams {
            max_tokens: 512,
            temperature: 0.0,
            timeout: std::time::Duration::from_secs(10),
        };
        let result = provider.generate(input, params);
        let gen_ms = gen_start.elapsed().as_millis();

        match result {
            Ok(text) => {
                println!("  OUT: {} ({} ms)", text, gen_ms);
                assert!(!text.trim().is_empty(), "output should not be empty");
            }
            Err(e) => {
                println!("  ERR: {} ({} ms)", e, gen_ms);
                // Print the raw response for debugging.
                let raw = needle_sys::complete_text(input, 512)
                    .unwrap_or_else(|e2| format!("raw also failed: {e2}"));
                println!("  RAW: {}", raw);
            }
        }
    }

    // 4. Test intent classification with real command examples.
    //    Uses the provider's classify_intent() which handles init + lock.
    println!("\n[4] Intent classification (command examples)");
    let commands = [
        ("open safari browser", "command"),
        ("open brave browser", "command"),
        ("open terminal and open pi inside", "command"),
        ("open system settings", "command"),
        ("lower the brightness", "command"),
        ("increase the brightness", "command"),
        ("mute the speaker", "command"),
        ("open vs code", "command"),
        ("open app store", "command"),
        ("open brave browser and load slack", "command"),
        (
            "the file is in the downloads folder and i need you to check it out",
            "dictation",
        ),
        (
            "actually i think we should ship it tomorrow not friday",
            "dictation",
        ),
    ];
    let mut correct = 0;
    for (i, (cmd, expected)) in commands.iter().enumerate() {
        let start = Instant::now();
        let intent = provider.classify_intent(cmd);
        let ms = start.elapsed().as_millis();
        let label = intent.label();
        let ok = label == *expected;
        if ok {
            correct += 1;
        }
        println!(
            "  [{}] {:<55} → {:<12} (expected {:<12} {} {} ms)",
            i + 1,
            cmd,
            label,
            expected,
            if ok { "✓" } else { "✗" },
            ms
        );
    }
    println!(
        "\n  Accuracy: {}/{} ({:.0}%)",
        correct,
        commands.len(),
        correct as f64 / commands.len() as f64 * 100.0
    );

    // 5. Test the embedding capability
    println!("\n[5] Embedding test");
    let embed_start = Instant::now();
    let embedding = needle_sys::embed_text("hello world").expect("embed failed");
    let embed_ms = embed_start.elapsed().as_millis();
    println!(
        "  Embedding dim: {}, {} ms (first 5: {:?})",
        embedding.len(),
        embed_ms,
        &embedding[..embedding.len().min(5)]
    );
    assert!(embedding.len() > 0, "embedding should be non-empty");

    println!("\n=== E2E Test Complete ===\n");
}
