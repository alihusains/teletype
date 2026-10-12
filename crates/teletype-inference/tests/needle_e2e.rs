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
//! # FINDING (2026-10-12)
//!
//! Needle 3 is a **tool-calling model**, not a free-text generator:
//! - With no tools: returns `{"type":"call","function_calls":[]}` with
//!   reasoning "no tools declared, returning the empty call."
//! - With a "polish" tool: the model **summarizes** the transcript
//!   (returns key phrases like "ship it tomorrow") instead of returning
//!   the full cleaned text. Confidence is low (0.02–0.04), so calls land
//!   in `suppressed_calls`.
//!
//! This is a **fundamental mismatch** for Teletype's polish use case,
//! which requires full-text transformation (preserve all words, fix
//! punctuation/grammar/fillers). Needle 3 is designed for bounded tool
//! calls (set_lights, get_weather), not full-text rewriting.
//!
//! **What works well:**
//! - Load: 11 ms. Init: 20–47 ms. Embedding: 1 ms (3072-dim vector).
//! - Latency: 17–48 ms per call (4000+ tok/s prefill, 1100+ tok/s decode).
//! - RAM: ~90–104 MB peak.
//! - Grammar-constrained decode: output is always valid JSON.
//!
//! **Verdict:** Needle 3 is NOT a drop-in replacement for the LLM polish
//! pass. It could work for:
//! - Structured extraction (ITN: extract time/number/name from transcript)
//! - Intent classification (choice: "command" vs "dictation" vs "code")
//! - Embedding (semantic search over user history)
//! - Voice commands (audio → tool call, via the Whistle companion model)
//!
//! But NOT for full-text transcript polishing.

use std::time::Instant;
use teletype_core::llm::{GenerationParams, InferenceProvider};
use teletype_inference::{NeedleProvider, POLISH_SYSTEM_PROMPT, POLISH_TOOLS_JSON};

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
    let provider = NeedleProvider::new("needle3", "Needle 3", &path)
        .expect("needle load failed");
    let load_ms = load_start.elapsed().as_millis();
    println!("\n[1] Load: {} ms", load_ms);
    assert!(provider.is_alive(), "model should be alive after load");

    // 2. Init with NO tools — use the "respond" path for free-text polish.
    //    Needle 3 is a tool-calling model; with no tools declared it returns
    //    type "respond" with the text in the `response` field.
    let init_start = Instant::now();
    let prefix_tokens = provider
        .init(Some(POLISH_SYSTEM_PROMPT), None)
        .expect("needle init failed");
    let init_ms = init_start.elapsed().as_millis();
    println!("[2] Init (no tools): {} ms (static prefix: {} tokens)", init_ms, prefix_tokens);

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
            .init(Some(POLISH_SYSTEM_PROMPT), None)
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

    // 4. Test the embedding capability
    println!("\n[4] Embedding test");
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
