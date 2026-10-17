//! Built-in model catalog.
//!
//! V1 ships the local GGUF models for polish and transforms (S1-mini,
//! Qwen, Gemma, Needle). Users can also point to any local
//! GGUF file (BYOM) — the catalog is a convenience, not a requirement.
//!
//! Weights are never re-hosted by Teletype; entries only link to the
//! publisher's official URLs. `requires_license_accept` marks entries whose
//! license needs an explicit checkbox before download.

use serde::Serialize;
/// A file exists and is not obviously truncated.
///
/// See [`CatalogEntry::is_downloaded`] for why this checks size and not the
/// hash.
fn has_plausible_size(path: &std::path::Path, expected_bytes: u64) -> bool {
    let Ok(md) = std::fs::metadata(path) else {
        return false; // missing
    };
    if !md.is_file() {
        return false;
    }
    let actual = md.len();
    if actual == 0 {
        return false; // an interrupted download leaves a 0-byte file
    }
    if expected_bytes == 0 {
        // No size known: existence and non-emptiness is all there is to check.
        return true;
    }
    // 5% tolerance covers the MB rounding in `is_downloaded`, and a file that
    // was still being appended to when we looked.
    let slack = expected_bytes / 20;
    actual + slack >= expected_bytes && actual <= expected_bytes + slack
}

/// One shard of a multi-file (split) GGUF model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogShard {
    pub file_name: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
    /// Lowercase hex SHA-256 of the full file.
    pub sha256: &'static str,
}

/// Measured quality verdict for a model, based on benchmark results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelVerdict {
    /// Our own model, measured on a different corpus.
    FirstParty,
    /// Measured 30% or better on the behaviour corpus.
    Recommended,
    /// Measured 14% to 29%. Usable, with a real failure mode.
    Mixed,
    /// Measured 1% to 13%.
    Unreliable,
    /// Produced no acceptable result in any test case.
    NotRecommended,
    /// We have not measured this model.
    NotTested,
}

impl ModelVerdict {
    /// Short label shown in parentheses after the model name in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::FirstParty => "Our model",
            Self::Recommended => "Recommended",
            Self::Mixed => "Mixed results",
            Self::Unreliable => "Unreliable",
            Self::NotRecommended => "Not recommended",
            Self::NotTested => "Not tested",
        }
    }

    /// One-line note explaining the verdict, shown as a tooltip or subtitle.
    pub fn note(self) -> &'static str {
        match self {
            Self::FirstParty => "",
            Self::Recommended => "Best in our tests",
            Self::Mixed => "Usable, with some failure modes",
            Self::Unreliable => "Rarely cleans dictation correctly",
            Self::NotRecommended => "Failed every test we ran",
            Self::NotTested => "Not tested by us",
        }
    }
}

/// One catalog entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    /// Approximate parameter class, for display.
    pub size_label: &'static str,
    /// Primary download URL (single-file models) or entrypoint shard URL.
    pub url: &'static str,
    /// Approximate download size in MB.
    pub size_mb: u32,
    pub description: &'static str,
    /// Lowercase hex SHA-256 for single-file models; `None` when unknown.
    #[serde(default)]
    pub sha256: Option<&'static str>,
    /// Alternate URL tried when the primary fails (single-file only).
    #[serde(default)]
    pub backup_url: Option<&'static str>,
    /// Attribution string shown in the UI, e.g. "S1-mini by Superwhisper".
    #[serde(default)]
    pub attribution: Option<&'static str>,
    /// SPDX-style license label, e.g. "Apache-2.0" or "Gemma Terms of Use".
    #[serde(default)]
    pub license_name: Option<&'static str>,
    /// Deep link to the full license text.
    #[serde(default)]
    pub license_url: Option<&'static str>,
    /// When true the UI must show an accept checkbox before downloading.
    #[serde(default)]
    pub requires_license_accept: bool,
    /// When true the UI shows a "Recommended" badge next to the model.
    #[serde(default)]
    pub recommended: bool,
    /// Measured quality verdict from benchmark runs.
    #[serde(default)]
    pub verdict: ModelVerdict,
    /// One-line note explaining the verdict.
    #[serde(default)]
    pub verdict_note: &'static str,
    /// Display priority in the model list (lower = higher on the list).
    /// Set explicitly so the ordering never depends on array position.
    #[serde(default)]
    pub sort_order: u32,
    /// Non-empty for multi-file (split) GGUFs. Install dir is `models/<id>/`
    /// and the llama-server entrypoint is the first shard.
    #[serde(default)]
    pub shards: &'static [CatalogShard],
}

impl CatalogEntry {
    /// True when the model is fully present on disk under `models_dir`.
    /// Whether every file this entry needs is present *and plausibly complete*.
    ///
    /// This used to be `is_file()`, so a zero-byte or half-downloaded GGUF
    /// counted as installed. Selecting that model then gave a model that cannot
    /// load, with nothing in the UI saying why, which is the same symptom as
    /// every other "I picked a model and nothing happened" bug.
    ///
    /// Size is checked rather than the SHA-256, because hashing every shard on
    /// every call would be several seconds of disk I/O on a screen refresh. The
    /// download path already verifies the hash before the file is moved into
    /// place, so by the time a file is here it is either correct or the result
    /// of a download that was interrupted, and a truncated file is short.
    pub fn is_downloaded(&self, models_dir: &std::path::Path) -> bool {
        if self.shards.is_empty() {
            // `size_mb` is rounded to the nearest MB, so allow 5% either way.
            let expected = u64::from(self.size_mb).saturating_mul(1_000_000);
            has_plausible_size(&models_dir.join(format!("{}.gguf", self.id)), expected)
        } else {
            self.shards.iter().all(|s| {
                has_plausible_size(&models_dir.join(self.id).join(s.file_name), s.size_bytes)
            })
        }
    }

    /// Path llama-server should be pointed at (`-m`).
    pub fn entrypoint(&self, models_dir: &std::path::Path) -> std::path::PathBuf {
        if self.shards.is_empty() {
            models_dir.join(format!("{}.gguf", self.id))
        } else {
            models_dir.join(self.id).join(self.shards[0].file_name)
        }
    }
}

/// The built-in catalog.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "s1-mini",
        name: "S1-mini",
        size_label: "0.6B",
        url: "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf",
        size_mb: 462,
        description:
            "Small open cleanup model, happiest in English. Pairs with Tone / Structure styles.",
        sha256: Some("3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634"),
        backup_url: None,
        attribution: Some("S1-mini by Superwhisper"),
        license_name: Some("Apache-2.0"),
        license_url: Some(
            "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/LICENSE",
        ),
        requires_license_accept: false,
        recommended: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Best for English: small, fast, and tuned for dictation cleanup",
        sort_order: 1,
        shards: &[],
    },
    CatalogEntry {
        id: "qwen2.5-3b",
        name: "Qwen2.5 3B",
        size_label: "3B",
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf",
        size_mb: 1900,
        description: "Best local model in our tests. Good for style profiles and transforms.",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen2.5 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Best in our tests, may follow dictated instructions",
        sort_order: 3,
        shards: &[],
    },
    CatalogEntry {
        id: "fast",
        name: "Fast (Qwen3 1.7B)",
        size_label: "1.7B",
        url: "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/qwen3-1.7b-q5_k_m.gguf",
        size_mb: 1400,
        description: "Quick local transforms. Good for Polish and short rewrites.",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen3 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Fast, good for Polish and short rewrites",
        sort_order: 4,
        shards: &[],
    },
    CatalogEntry {
        id: "qwen3-0.6b",
        name: "Qwen3 0.6B",
        size_label: "0.6B",
        url: "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/qwen3-0.6b-q4_k_m.gguf",
        size_mb: 400,
        description: "Very small and fast. Good for low-RAM Macs.",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen3 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Scored well, very small download",
        sort_order: 5,
        shards: &[],
    },
    CatalogEntry {
        id: "quality",
        name: "Quality (Qwen3 4B)",
        size_label: "4B",
        url: "https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/qwen3-4b-q4_k_m.gguf",
        size_mb: 2600,
        description: "Better quality for Professional and Prompt Engineer transforms.",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen3 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen3-4B-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Better quality for complex transforms",
        sort_order: 6,
        shards: &[],
    },
    CatalogEntry {
        id: "qwen2.5-7b",
        name: "Qwen2.5 7B",
        size_label: "7B",
        url: "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m.gguf",
        size_mb: 4900,
        description: "Highest quality local model. Slower on older Macs.",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen2.5 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Resists dictated instructions, sometimes drops words",
        sort_order: 7,
        shards: &[],
    },
    CatalogEntry {
        id: "gemma2-2b",
        name: "Gemma 2 2B",
        size_label: "2B",
        url: "https://huggingface.co/google/gemma-2-2b-it-GGUF/resolve/main/gemma-2-2b-it-Q4_K_M.gguf",
        size_mb: 1500,
        description: "Google's small model. Mixed results with non-English dictation.",
        sha256: None,
        backup_url: None,
        attribution: Some("Gemma 2 by Google (Google License)"),
        license_name: Some("Google Gemma Terms of Use"),
        license_url: Some("https://ai.google.dev/gemma/terms"),
        requires_license_accept: false,
        recommended: false,
        verdict: ModelVerdict::Mixed,
        verdict_note: "Mixed results, often mishandles other languages",
        sort_order: 8,
        shards: &[],
    },
    CatalogEntry {
        id: "needle3",
        name: "Needle 3 (Cactus Compute)",
        size_label: "121M",
        url: "https://huggingface.co/Cactus-Compute/needle3/resolve/main/needle3.cact",
        size_mb: 35,
        description:
            "On-device tool-calling + extraction + embedding. NOT for polish (use S1-mini).",
        sha256: None,
        backup_url: None,
        attribution: Some("Needle 3 by Cactus Compute (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Cactus-Compute/needle3/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: true,
        verdict: ModelVerdict::NotTested,
        verdict_note: "Extraction, classification, embedding. Not for polish.",
        sort_order: 0,
        shards: &[],
    },
    CatalogEntry {
        id: "neudecide-q4",
        name: "NeuDecide Q4 (decider)",
        size_label: "55.5M",
        url: "https://huggingface.co/neuphonic/neudecide/resolve/main/config.json",
        size_mb: 41,
        description: "Speech-to-tool-call decider for Command Mode (ONNX, runs in-process).",
        sha256: None,
        backup_url: None,
        attribution: Some("NeuDecide by neuphonic (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/neuphonic/neudecide/blob/main/README.md"),
        requires_license_accept: false,
        recommended: true,
        verdict: ModelVerdict::NotTested,
        verdict_note: "Default Command Mode decider (Addendum A); not measured on our corpus yet.",
        sort_order: 9,
        shards: &[
            CatalogShard {
                file_name: "config.json",
                url: "https://huggingface.co/neuphonic/neudecide/resolve/main/config.json",
                size_bytes: 1_002,
                sha256: "a1eb5783b7843541b193258c1296d616a4f3f9f5893745bac74c5ef19b2e53a0",
            },
            CatalogShard {
                file_name: "audio_encoder_q4.onnx",
                url: "https://huggingface.co/neuphonic/neudecide/resolve/main/audio_encoder_q4.onnx",
                size_bytes: 22_519_972,
                sha256: "2ac5c48e1766382736ed8dd4d468568ee583536550fb741ac187219f8cd8da9b",
            },
            CatalogShard {
                file_name: "tool_encoder_q4.onnx",
                url: "https://huggingface.co/neuphonic/neudecide/resolve/main/tool_encoder_q4.onnx",
                size_bytes: 10_090_217,
                sha256: "4a5075ee2868d90ea4be3234eae04001be6886c9730e8746421fa17e84313d85",
            },
            CatalogShard {
                file_name: "decoder_step_q4.onnx",
                url: "https://huggingface.co/neuphonic/neudecide/resolve/main/decoder_step_q4.onnx",
                size_bytes: 9_951_225,
                sha256: "ee68bbb4bbca552b909b41cb1fdbfdf0f9f3c852561e18036044ee2ff461e50d",
            },
            CatalogShard {
                file_name: "tokenizer.json",
                url: "https://huggingface.co/neuphonic/neudecide/resolve/main/tokenizer.json",
                size_bytes: 228_594,
                sha256: "2b31f2b79506cca36b3c63f0e8549e8401dd8d768175c57aa1e67432d69dee0d",
            },
            CatalogShard {
                file_name: "tokenizer.model",
                url: "https://huggingface.co/neuphonic/neudecide/resolve/main/tokenizer.model",
                size_bytes: 124_892,
                sha256: "b39d6892d3b27e66a1782ee624d965e5e656c03e9226cbe036a80389090947ad",
            },
        ],
    },
];

/// Finds a catalog entry by id.
pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_known_ids() {
        for id in [
            "fast",
            "quality",
            "s1-mini",
            "qwen2.5-3b",
            "needle3",
            "neudecide-q4",
        ] {
            assert!(find(id).is_some(), "missing {id}");
        }
        assert!(find("nope").is_none());
    }

    #[test]
    fn s1_mini_has_checksum_and_no_license_gate() {
        let e = find("s1-mini").unwrap();
        assert!(e.sha256.is_some());
        assert!(!e.requires_license_accept);
        assert!(e.shards.is_empty());
        assert_eq!(e.attribution, Some("S1-mini by Superwhisper"));
    }
    #[test]
    fn neudecide_q4_pins_all_six_files() {
        let e = find("neudecide-q4").unwrap();
        // Six pinned artifacts: config + 3 ONNX graphs + 2 tokenizers.
        assert_eq!(e.shards.len(), 6);
        for s in e.shards {
            assert_eq!(
                s.sha256.len(),
                64,
                "sha256 of {} must be 64 hex chars",
                s.file_name
            );
            assert!(s
                .url
                .starts_with("https://huggingface.co/neuphonic/neudecide/resolve/main/"));
            assert!(s.size_bytes > 0);
        }
        // Pinned total: 6 shards = 42,915,902 bytes (≈ 40.9 MiB); size_mb
        // rounds the MiB display (same convention as s1-mini: 461.78 → 462).
        let total: u64 = e.shards.iter().map(|s| s.size_bytes).sum();
        assert_eq!(total, 42_915_902, "neudecide-q4 total byte size drifted");
        assert_eq!(e.size_mb, 41);
        // Entry contract: entrypoint is the first shard file; the decider
        // loads the parent dir (<dir>/neudecide-q4) where all 6 shards
        // install (is_downloaded + the downloader use models_dir/<id>).
        let dir = std::path::Path::new("/models");
        assert_eq!(
            e.entrypoint(dir),
            dir.join("neudecide-q4").join("config.json")
        );
        assert!(!e.is_downloaded(dir));
    }

    /// A synthetic two-shard entry: exercises the multi-shard install path
    /// without depending on any catalog model.
    fn multi_shard_entry() -> CatalogEntry {
        const SHARDS: &[CatalogShard] = &[
            CatalogShard {
                file_name: "part-00001-of-00002.gguf",
                url: "https://example.invalid/part-00001-of-00002.gguf",
                size_bytes: 8,
                sha256: "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f",
            },
            CatalogShard {
                file_name: "part-00002-of-00002.gguf",
                url: "https://example.invalid/part-00002-of-00002.gguf",
                size_bytes: 4,
                sha256: "1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f",
            },
        ];
        CatalogEntry {
            id: "test-multi",
            name: "Test multi-shard",
            size_label: "t",
            url: "https://example.invalid/test-multi",
            size_mb: 0,
            description: "synthetic multi-shard entry for tests",
            sha256: None,
            backup_url: None,
            attribution: None,
            license_name: None,
            license_url: None,
            requires_license_accept: false,
            recommended: false,
            verdict: ModelVerdict::NotTested,
            verdict_note: "",
            sort_order: 0,
            shards: SHARDS,
        }
    }

    #[test]
    fn entrypoint_paths_for_single_and_multi() {
        let single = find("s1-mini").unwrap();
        let multi = multi_shard_entry();
        let dir = std::path::Path::new("/models");
        assert_eq!(single.entrypoint(dir), dir.join("s1-mini.gguf"));
        assert_eq!(
            multi.entrypoint(dir),
            dir.join("test-multi").join("part-00001-of-00002.gguf")
        );
        // Nothing is downloaded in a fresh dir.
        assert!(!single.is_downloaded(dir));
        assert!(!multi.is_downloaded(dir));
    }

    /// A file that is present but truncated is not "downloaded".
    ///
    /// This was `is_file()`, so a zero-byte GGUF from an interrupted download
    /// counted as installed. The model screen then showed it as downloaded,
    /// selecting it produced a model that could not load, and the only symptom
    /// was "no model loaded" with nothing pointing at the cause.
    #[test]
    fn a_truncated_file_is_not_downloaded() {
        let dir = std::env::temp_dir().join(format!("teletype-cat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let single = find("s1-mini").unwrap();
        let path = single.entrypoint(&dir);

        // Zero bytes: what an interrupted download leaves behind.
        std::fs::write(&path, b"").unwrap();
        assert!(
            !single.is_downloaded(&dir),
            "a 0-byte model file must not count as downloaded"
        );

        // A tenth of the file: a partial download, the case a plain existence
        // check cannot see.
        let tenth = u64::from(single.size_mb) * 100_000;
        std::fs::write(&path, vec![0u8; tenth as usize]).unwrap();
        assert!(
            !single.is_downloaded(&dir),
            "a truncated model file must not count as downloaded"
        );

        // A directory in place of the file is not a model either.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        assert!(!single.is_downloaded(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Multi-shard entries need every shard, and every shard has to be
    /// complete. One missing shard, or one short one, is not downloaded.
    #[test]
    fn a_multi_shard_entry_needs_every_shard_at_full_size() {
        let dir = std::env::temp_dir().join(format!("teletype-shards-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let multi = multi_shard_entry();
        let shard_dir = dir.join(multi.id);
        std::fs::create_dir_all(&shard_dir).unwrap();

        // No shards at all.
        assert!(!multi.is_downloaded(&dir));

        // All shards present but each 0 bytes.
        for s in multi.shards {
            std::fs::write(shard_dir.join(s.file_name), b"").unwrap();
        }
        assert!(!multi.is_downloaded(&dir), "0-byte shards are not a model");

        // Every shard at its exact catalog size: now it counts.
        for s in multi.shards {
            std::fs::write(
                shard_dir.join(s.file_name),
                vec![0u8; s.size_bytes as usize],
            )
            .unwrap();
        }
        assert!(multi.is_downloaded(&dir));

        // Drop one shard again.
        std::fs::remove_file(shard_dir.join(multi.shards[1].file_name)).unwrap();
        assert!(
            !multi.is_downloaded(&dir),
            "one missing shard is not enough"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
