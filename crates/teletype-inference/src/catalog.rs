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
    pub fn is_downloaded(&self, models_dir: &std::path::Path) -> bool {
        if self.shards.is_empty() {
            models_dir.join(format!("{}.gguf", self.id)).is_file()
        } else {
            self.shards
                .iter()
                .all(|s| models_dir.join(self.id).join(s.file_name).is_file())
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
        sort_order: 2,
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
        sort_order: 3,
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
        sort_order: 4,
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
        sort_order: 1,
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
}
