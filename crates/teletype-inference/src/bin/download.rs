//! Downloads a model from the catalog to the local models directory.
//!
//! Usage: teletype-model-download <model-id> [dest-dir]
//!
//! This is an explicit, user-initiated operation — the app never downloads
//! models silently.

use std::{fs, io::Write, path::PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: teletype-model-download <model-id> [dest-dir]");
        eprintln!(
            "Available: {}",
            teletype_inference::catalog::CATALOG
                .iter()
                .map(|e| e.id)
                .collect::<Vec<_>>()
                .join(", ")
        );
        std::process::exit(1);
    }
    let id = &args[0];
    let entry = teletype_inference::catalog::find(id.as_str()).unwrap_or_else(|| {
        eprintln!(
            "Unknown model '{id}'. Available: {}",
            teletype_inference::catalog::CATALOG
                .iter()
                .map(|e| e.id)
                .collect::<Vec<_>>()
                .join(", ")
        );
        std::process::exit(1);
    });

    let dest_dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("teletype")
            .join("models")
    });
    fs::create_dir_all(&dest_dir).expect("couldn't create models dir");
    let dest = dest_dir.join(format!("{}.gguf", entry.id));

    if dest.exists() {
        println!("Already downloaded: {}", dest.display());
        return;
    }

    println!("Downloading {} (~{} MB)…", entry.name, entry.size_mb);
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(entry.url)
        .header("User-Agent", "teletype/0.1")
        .send()
        .expect("download failed");

    if !response.status().is_success() {
        eprintln!(
            "HTTP {}: {}",
            response.status(),
            response.status().canonical_reason().unwrap_or("")
        );
        std::process::exit(1);
    }
    let _total = response
        .content_length()
        .unwrap_or((entry.size_mb as u64) * 1024 * 1024);

    let bytes = response.bytes().expect("read error");
    let total = bytes.len() as u64;

    let mut file = fs::File::create(&dest).expect("couldn't create file");
    let mut last_print = std::time::Instant::now();

    // Write in 64KB chunks with progress.
    let mut offset = 0;
    while offset < bytes.len() {
        let end = (offset + 64 * 1024).min(bytes.len());
        file.write_all(&bytes[offset..end]).expect("write error");
        offset = end;
        if last_print.elapsed().as_secs() >= 1 {
            let pct = offset as f64 / total.max(1) as f64 * 100.0;
            println!(
                "\r  {:.0}% ({} MB / {} MB)",
                pct,
                offset as u64 / 1024 / 1024,
                total / 1024 / 1024
            );
            last_print = std::time::Instant::now();
        }
    }
    file.flush().expect("flush error");
    println!("\nDone: {}", dest.display());
}
