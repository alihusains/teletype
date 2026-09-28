//! Downloads a model from the catalog to the local models directory.
//!
//! Usage: teletype-model-download <model-id> [dest-dir]
//!
//! This is an explicit, user-initiated operation — the app never downloads
//! models silently. Multi-shard entries (EG-1) require `--accept-license`.

use std::{fs, path::PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: teletype-model-download <model-id> [dest-dir] [--accept-license]");
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
    let accept_license = args.iter().any(|a| a == "--accept-license");
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let id = positional[0];
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

    if entry.requires_license_accept && !accept_license {
        eprintln!(
            "Model '{id}' requires a license acceptance.\nLicense: {}",
            entry.license_url.unwrap_or("(unknown)")
        );
        eprintln!("Re-run with --accept-license after reading it.");
        std::process::exit(1);
    }

    let dest_dir = positional
        .get(1)
        .map(|s| PathBuf::from(s.as_str()))
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("teletype")
                .join("models")
        });
    fs::create_dir_all(&dest_dir).expect("couldn't create models dir");

    if entry.is_downloaded(&dest_dir) {
        println!(
            "Already downloaded: {}",
            entry.entrypoint(&dest_dir).display()
        );
        return;
    }

    println!("Downloading {} (~{} MB)…", entry.name, entry.size_mb);
    match teletype_inference::download_entry(entry, &dest_dir) {
        Ok(path) => println!("Done: {}", path.display()),
        Err(e) => {
            eprintln!("Download failed: {e}");
            std::process::exit(1);
        }
    }
}
