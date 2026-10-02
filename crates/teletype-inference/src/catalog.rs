//! Built-in model catalog.
//!
//! V1 ships two Qwen3-class GGUF models plus the polish models (S1-mini,
//! EG-1). Users can also point to any local
//! GGUF file (BYOM) — the catalog is a convenience, not a requirement.
//!
//! Weights are never re-hosted by Teletype; entries only link to the
//! publisher's official URLs. EG-1 requires an explicit license acceptance
//! before download (see `requires_license_accept`).

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
    /// SPDX-style license label, e.g. "Apache-2.0", "EG-1 Community License".
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

const EG1_BASE: &str = "https://models.enviouslabs.co/eg1/eg1-1.2-c003/";

/// EG-1 1.2 split GGUF shards (all required). We link only; never re-host.
const EG1_SHARDS: &[CatalogShard] = &[
    CatalogShard {
        file_name: "eg1-1.2-c003-00001-of-00008.gguf",
        url: "https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00001-of-00008.gguf",
        size_bytes: 399_884_640,
        sha256: "8f05b91acb93ed9c0ef833d617f4cefb492f697c4fe783f6ba26ce4add14e3c0",
    },
    CatalogShard {
        file_name: "eg1-1.2-c003-00002-of-00008.gguf",
        url: "https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00002-of-00008.gguf",
        size_bytes: 395_007_008,
        sha256: "011892fa153cf95f3dc68c049860ad35c752a990f582e4b33c974205df2486fa",
    },
    CatalogShard {
        file_name: "eg1-1.2-c003-00003-of-00008.gguf",
        url: "https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00003-of-00008.gguf",
        size_bytes: 393_972_896,
        sha256: "5769143e7fc28cf4b052237da617cfcbebf2f16bf1500acc4c7ed483fbc18a6f",
    },
    CatalogShard {
        file_name: "eg1-1.2-c003-00004-of-00008.gguf",
        url: "https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00004-of-00008.gguf",
        size_bytes: 397_618_336,
        sha256: "52724cb4812115d993c5f4e4f4ae5e87a44e005fd05b51e14bf427d353dd9d78",
    },
    CatalogShard {
        file_name: "eg1-1.2-c003-00005-of-00008.gguf",
        url: "https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00005-of-00008.gguf",
        size_bytes: 389_508_864,
        sha256: "9b414a065c40e25dea077ae9fe8198ae9c13965cb5df5e997df0580aef452450",
    },
    CatalogShard {
        file_name: "eg1-1.2-c003-00006-of-00008.gguf",
        url: "https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00006-of-00008.gguf",
        size_bytes: 388_606_432,
        sha256: "00ccd4bcf1507db1ca57252ed3465672a3acd1ec49eb450512183bd20e67f25e",
    },
    CatalogShard {
        file_name: "eg1-1.2-c003-00007-of-00008.gguf",
        url: "https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00007-of-00008.gguf",
        size_bytes: 397_168_384,
        sha256: "3a7b065f44ab398f9eb9656aa263f43a9be1acd6af55a0790c7960162675e729",
    },
    CatalogShard {
        file_name: "eg1-1.2-c003-00008-of-00008.gguf",
        url: "https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00008-of-00008.gguf",
        size_bytes: 127_746_048,
        sha256: "91160e7d004150b44a360a7db69237b5405f34d29855c15480324a4abfdf5c0c",
    },
];

/// The built-in catalog.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "s1-mini",
        name: "S1-mini",
        size_label: "0.6B",
        url: "https://models.enviouslabs.co/s1/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf",
        size_mb: 462,
        description:
            "Small open cleanup model, happiest in English. Pairs with Tone / Structure styles.",
        sha256: Some("3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634"),
        backup_url: Some(
            "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf",
        ),
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
        id: "eg-1",
        name: "EG-1 (Envious Labs)",
        size_label: "4B",
        url: EG1_BASE,
        size_mb: 2758,
        description:
            "Dictation cleanup fine-tune from Envious Labs. 8 shards; requires accepting their license.",
        sha256: None,
        backup_url: None,
        attribution: Some("EG-1 by Envious Labs (not affiliated with Teletype)"),
        license_name: Some("EG-1 Community Model License 1.0"),
        license_url: Some("https://models.enviouslabs.co/eg1/EG-1-MODEL-LICENSE.txt"),
        requires_license_accept: true,
        recommended: true,
        verdict: ModelVerdict::FirstParty,
        verdict_note: "Highest-quality local polish; 8 shards, ~2.7 GB",
        sort_order: 2,
        shards: EG1_SHARDS,
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
        for id in ["fast", "quality", "s1-mini", "eg-1"] {
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
    fn eg1_is_license_gated_with_eight_shards() {
        let e = find("eg-1").unwrap();
        assert!(e.requires_license_accept);
        assert_eq!(e.shards.len(), 8);
        assert!(e.license_url.is_some());
        // Every shard must declare a 64-char hex digest.
        for s in e.shards {
            assert_eq!(s.sha256.len(), 64);
            assert!(s.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(s.url.starts_with("https://"));
        }
        // Total size should match ~2.69 GB from the roadmap.
        let total: u64 = e.shards.iter().map(|s| s.size_bytes).sum();
        assert_eq!(total, 2_889_512_608);
    }

    #[test]
    fn entrypoint_paths_for_single_and_multi() {
        let single = find("s1-mini").unwrap();
        let multi = find("eg-1").unwrap();
        let dir = std::path::Path::new("/models");
        assert_eq!(single.entrypoint(dir), dir.join("s1-mini.gguf"));
        assert_eq!(
            multi.entrypoint(dir),
            dir.join("eg-1").join("eg1-1.2-c003-00001-of-00008.gguf")
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
        let eg1 = find("eg-1").unwrap();
        let shard_dir = dir.join(eg1.id);
        std::fs::create_dir_all(&shard_dir).unwrap();

        // No shards at all.
        assert!(!eg1.is_downloaded(&dir));

        // All shards present but each 0 bytes.
        for s in eg1.shards {
            std::fs::write(shard_dir.join(s.file_name), b"").unwrap();
        }
        assert!(!eg1.is_downloaded(&dir), "0-byte shards are not a model");

        // Every shard at its exact catalog size: now it counts.
        for s in eg1.shards {
            std::fs::write(
                shard_dir.join(s.file_name),
                vec![0u8; s.size_bytes as usize],
            )
            .unwrap();
        }
        assert!(eg1.is_downloaded(&dir));

        // Drop one shard again.
        std::fs::remove_file(shard_dir.join(eg1.shards[7].file_name)).unwrap();
        assert!(!eg1.is_downloaded(&dir), "one missing shard is not enough");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
