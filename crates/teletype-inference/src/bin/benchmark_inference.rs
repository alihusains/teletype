//! Inference engine benchmark binary.
//! Run with: cargo run -p teletype-inference --bin benchmark_inference

use std::time::{Duration, Instant};

use teletype_core::llm::{GenerationParams, InferenceProvider};
use teletype_inference::{MagnitudeConfig, MagnitudeProvider, ServerProvider};

const TEST_PROMPTS: &[&str] = &[
    "hey john how are you doing today",
    "the quarterly report is due friday please review it and send feedback",
    "can you please schedule a meeting with the team for next tuesday at 2pm",
    "remember to buy milk eggs and bread on the way home",
    "the new feature implementation looks good but we need to add error handling",
];

const ITERATIONS: usize = 3;

fn benchmark_provider<P: InferenceProvider + ?Sized>(
    name: &str,
    provider: &P,
    model_name: &str,
) -> BenchmarkResult {
    println!("\n--- Benchmarking {} ({}) ---", name, model_name);
    
    let mut latencies = Vec::new();
    let mut token_counts = Vec::new();
    let mut success_count = 0;
    let mut total_chars = 0;
    
    for prompt in TEST_PROMPTS {
        for i in 0..ITERATIONS {
            let params = GenerationParams {
                max_tokens: 300,
                temperature: 0.0,
                timeout: Duration::from_secs(30),
            };
            
            let start = Instant::now();
            match provider.generate(prompt, params) {
                Ok(output) => {
                    let latency = start.elapsed();
                    let tokens = estimate_tokens(&output);
                    
                    println!("  ✓ Iteration {}: {}ms, ~{} tokens: {}...",
                        i + 1,
                        latency.as_millis(),
                        tokens,
                        output.chars().take(50).collect::<String>()
                    );
                    
                    latencies.push(latency.as_millis() as u64);
                    token_counts.push(tokens);
                    total_chars += output.len();
                    success_count += 1;
                }
                Err(e) => {
                    println!("  ✗ Iteration {} failed: {}", i + 1, e);
                }
            }
        }
    }
    
    if success_count == 0 {
        return BenchmarkResult {
            name: name.to_string(),
            model: model_name.to_string(),
            avg_latency_ms: 0,
            avg_tokens: 0,
            tokens_per_sec: 0.0,
            success_rate: 0.0,
            total_chars: 0,
        };
    }
    
    let avg_latency = latencies.iter().sum::<u64>() / latencies.len() as u64;
    let avg_tokens = token_counts.iter().sum::<u64>() / token_counts.len() as u64;
    let tokens_per_sec = if avg_latency > 0 {
        (avg_tokens as f64 * 1000.0) / avg_latency as f64
    } else {
        0.0
    };
    let success_rate = (success_count as f64 / (TEST_PROMPTS.len() * ITERATIONS) as f64) * 100.0;
    
    println!("  Average: {}ms latency, {} tokens, {:.1} tok/s, {:.1}% success",
        avg_latency, avg_tokens, tokens_per_sec, success_rate);
    
    BenchmarkResult {
        name: name.to_string(),
        model: model_name.to_string(),
        avg_latency_ms: avg_latency,
        avg_tokens,
        tokens_per_sec,
        success_rate,
        total_chars,
    }
}

fn estimate_tokens(text: &str) -> u64 {
    // Rough estimation: ~4 chars per token for English
    (text.len() as f64 / 4.0).ceil() as u64
}

struct BenchmarkResult {
    name: String,
    model: String,
    avg_latency_ms: u64,
    avg_tokens: u64,
    tokens_per_sec: f64,
    success_rate: f64,
    total_chars: usize,
}

fn main() {
    println!("=== Teletype Inference Engine Benchmark ===");
    println!("Prompts: {}", TEST_PROMPTS.len());
    println!("Iterations per prompt: {}", ITERATIONS);
    
    let mut results = Vec::new();
    
    // Models directory (macOS app data)
    let models_dir = dirs::home_dir()
        .map(|h| h.join("Library/Application Support/com.teletype.app/models"))
        .unwrap_or_else(|| std::path::PathBuf::from("target/debug/models"));
    
    println!("Models directory: {:?}", models_dir);
    
    // Try Magnitude first
    println!("\n=== Testing Magnitude ===");
    let mag_config = MagnitudeConfig::default();
    let mag_provider = MagnitudeProvider::new(mag_config);
    
    // Test connection first
    match mag_provider.test_connection() {
        Ok(msg) => {
            println!("✓ Magnitude connected: {}", msg);
            // Extract model name from the message or use default
            let model = "magnitude-model"; // Would need to parse from available models
            let result = benchmark_provider("Magnitude", &mag_provider, model);
            results.push(result);
        }
        Err(e) => {
            println!("✗ Magnitude not available: {}", e);
            println!("  Make sure Magnitude Desktop is running with a model loaded");
        }
    }
    
    // Try llama.cpp (ServerProvider) - test S1-mini
    println!("\n=== Testing llama.cpp ===");
    let s1_path = models_dir.join("s1-mini.gguf");
    if s1_path.exists() {
        println!("Found S1-mini at {:?}", &s1_path);
        let provider = ServerProvider::new("s1-mini", "S1-mini", &s1_path);
        match provider.warm_up() {
            Ok(_) => {
                println!("✓ llama.cpp warmed up for S1-mini");
                let result = benchmark_provider("llama.cpp", &provider, "s1-mini");
                results.push(result);
            }
            Err(e) => {
                println!("✗ llama.cpp warm-up failed for S1-mini: {}", e);
            }
        }
    } else {
        println!("S1-mini not found at {:?}", s1_path);
    }
    
    // Try EG-1
    let eg1_path = models_dir.join("eg-1").join("eg1-1.2-c003-00001-of-00008.gguf");
    if eg1_path.exists() {
        println!("\nFound EG-1 at {:?}", &eg1_path);
        let provider = ServerProvider::new("eg-1", "EG-1", &eg1_path);
        match provider.warm_up() {
            Ok(_) => {
                println!("✓ llama.cpp warmed up for EG-1");
                let result = benchmark_provider("llama.cpp", &provider, "eg-1");
                results.push(result);
            }
            Err(e) => {
                println!("✗ llama.cpp warm-up failed for EG-1: {}", e);
            }
        }
    } else {
        println!("EG-1 not found at {:?}", eg1_path);
    }
    
    // Summary
    println!("\n=== BENCHMARK SUMMARY ===");
    println!("{:<20} {:<20} {:>10} {:>10} {:>12} {:>10}", 
        "Engine", "Model", "Latency", "Tokens", "Tok/s", "Success%");
    println!("{}", "-".repeat(90));
    
    for r in &results {
        println!("{:<20} {:<20} {:>10}ms {:>10} {:>12.1} {:>9.1}%",
            r.name, r.model, r.avg_latency_ms, r.avg_tokens, r.tokens_per_sec, r.success_rate);
    }
    
    if results.len() >= 2 {
        let llama = results.iter().find(|r| r.name == "llama.cpp");
        let mag = results.iter().find(|r| r.name == "Magnitude");
        
        if let (Some(l), Some(m)) = (llama, mag) {
            println!("\n=== COMPARISON ===");
            let speedup = l.avg_latency_ms as f64 / m.avg_latency_ms as f64;
            let tok_ratio = m.tokens_per_sec / l.tokens_per_sec;
            println!("Magnitude vs llama.cpp:");
            println!("  Latency ratio: {:.2}x ({} faster)", speedup, if speedup > 1.0 { "Magnitude" } else { "llama.cpp" });
            println!("  Throughput ratio: {:.2}x ({} faster)", tok_ratio, if tok_ratio > 1.0 { "Magnitude" } else { "llama.cpp" });
        }
    }
}