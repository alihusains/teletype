//! Built-in model catalog.
//!
//! V1 ships the local GGUF models for polish and transforms (S1-mini,
//! Qwen, Gemma, Needle). Users can also point to any local
//! GGUF file (BYOM) — the catalog is a convenience, not a requirement.
//!
//! Most weights are only linked to the publisher's official URLs. The one
//! exception is NeuDecide, mirrored verbatim at
//! `github.com/alihusains/teletype-models` (Apache-2.0 permits redistribution;
//! upstream HF is gated behind a login form — the mirror is the primary URL,
//! upstream stays as `backup_url`). `requires_license_accept` marks entries
//! whose license needs an explicit checkbox before download.

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
    /// What this row is worth choosing for, shown as a chip beside the name.
    /// Empty when `description` already makes the job obvious.
    #[serde(default)]
    pub purpose: &'static str,
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
    /// False for models the app can never load as a polish or transform
    /// engine: the Command Mode decider and on-device extraction tools. They
    /// stay downloadable, but they must not sit in the polish list or be
    /// selectable, because choosing one makes every preset a no-op.
    pub polish_capable: bool,
    /// True when a host with no GPU-accelerated llama-server should be shown
    /// this model as a workable choice.
    ///
    /// The axis is not the OS brand. Every GGUF here downloads and loads on
    /// Windows and macOS alike; what differs is the build of the bundled
    /// engine. `scripts/build-llama-server.sh:118` compiles llama.cpp with
    /// `-DGGML_METAL=ON`, the macOS release jobs ship that build, and the
    /// Windows job builds llama-server CPU-only
    /// (`.github/workflows/release.yml:631`, "Build llama-server (CPU,
    /// static)"). An Intel Mac keeps the Metal flag but is documented in the
    /// same workflow as "not competitive" on integrated graphics.
    ///
    /// The cutoff is a parameter-count heuristic, not a measured per-host
    /// latency: sub-billion-parameter rows tokenize and generate fast enough
    /// that a CPU host still returns a polish in seconds, while 1.7B and up can
    /// add tens of seconds per take. The evals measured quality on Apple
    /// Silicon, so the CPU-side ranking inside this set is unmeasured; what is
    /// safe to claim is that these rows are the ones a CPU host can carry.
    pub runs_on_cpu: bool,
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

    /// True when this host can work with the model: see
    /// [`CatalogEntry::runs_on_cpu`] for what the flag means and what it does
    /// not claim. Used for the first-run shortlist and the "this computer"
    /// grouping, never to hide a row from Settings, where a user with a 32 GB
    /// Windows box is entitled to pick the 7B model and wait.
    pub fn runs_on_host(&self, gpu_accelerated: bool) -> bool {
        gpu_accelerated || self.runs_on_cpu
    }
}

/// Whether this machine's bundled llama-server has a GPU backend to run with.
///
/// The Apple Silicon macOS build is the only one that gets Metal *and* Metal
/// that is fast enough to matter: Windows ships a CPU-only llama-server and the
/// Intel macOS job is documented as not competitive. Called
/// `gpu_accelerated` rather than `is_mac` because that is the property the
/// catalog keys on, and a future CUDA or Vulkan build flips this one function
/// instead of every platform comparison above it.
pub fn host_gpu_accelerated() -> bool {
    matches!(
        teletype_speech::catalog::host_platform(),
        teletype_speech::catalog::Platform::MacosAppleSilicon { .. }
    )
}

/// How many polish rows a first-run surface may show.
pub const POLISH_SHORTLIST_MAX: usize = 3;

/// The polish rows worth offering someone who has never polished text here,
/// best first, for the engine build this host actually runs.
///
/// Written as two literal lists rather than derived by ranking the catalog, so
/// a change to `sort_order` or a new row cannot silently re-recommend a model
/// nobody looked at. Both lists start at s1-mini, which is the shipped default
/// and the smallest recommended row, so the first card a user ever sees is a
/// model already known to behave.
///
/// A CPU-only host gets two rows: the recommended list there has no third entry
/// that is both measured good and small enough to carry, and padding it with a
/// 1.7B model to hit three would recommend a model the function exists to
/// avoid recommending.
///
/// Every id is re-checked against [`CatalogEntry::runs_on_host`] and
/// [`CatalogEntry::polish_capable`] as the list is built, so the promise "no row
/// reaches a host that cannot use it" holds even if a flag below is later
/// flipped.
pub fn polish_shortlist_ids(gpu_accelerated: bool) -> Vec<&'static str> {
    const CPU: &[&str] = &["s1-mini", "speakoflow-mini"];
    const ACCELERATED: &[&str] = &["s1-mini", "qwen3.5-2b"];

    let ids = if gpu_accelerated { ACCELERATED } else { CPU };
    ids.iter()
        .copied()
        .filter(|id| find(id).is_some_and(|e| e.polish_capable && e.runs_on_host(gpu_accelerated)))
        .take(POLISH_SHORTLIST_MAX)
        .collect()
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
        purpose: "Dictation cleanup",
        sha256: Some("3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634"),
        backup_url: None,
        attribution: Some("S1-mini by Superwhisper"),
        license_name: Some("Apache-2.0"),
        license_url: Some(
            "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/LICENSE",
        ),
        requires_license_accept: false,
        recommended: true,
        polish_capable: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Best for English: small, fast, and tuned for dictation cleanup",
        runs_on_cpu: true,
        sort_order: 1,
        shards: &[],
    },
    CatalogEntry {
        id: "qwen3.5-2b",
        name: "Qwen3.5 2B",
        size_label: "2B",
        url: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/f6d5376be1edb4d416d56da11e5397a961aca8ae/Qwen3.5-2B-IQ4_XS.gguf",
        size_mb: 1173,
        description:
            "The all-rounder: cleans up, tightens, rewords and structures dictation, honors the Polish settings, and runs Prompt Engineer.",
        purpose: "Polish + Prompt Engineer",
        sha256: Some("3639f34b5ca22aa1c51f3616566eae8c355111554f6924ad97ee2652ed11c1cd"),
        backup_url: None,
        attribution: Some("Qwen3.5 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen3.5-2B/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        polish_capable: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Best all-rounder: tightens, rewords and structures Polish, plus Prompt Engineer",
        runs_on_cpu: false,
        sort_order: 2,
        shards: &[],
    },
    CatalogEntry {
        id: "speakoflow-mini",
        name: "SpeakoFlow Mini",
        size_label: "0.8B",
        url: "https://huggingface.co/SpeakoFlow/speakoflow-mini/resolve/main/SpeakoFlow-Mini-0.8B-Q8_0.gguf",
        size_mb: 834,
        description:
            "Purpose-built dictation cleanup: fixes grammar, spelling, fillers and spoken symbols; leaves correct text and names/numbers untouched. A cleanup model, not a rewriter.",
        purpose: "Dictation cleanup",
        sha256: Some("696769bb6911f51bc231b112926e934cf7bfc760e6cdfa24212907bc5ad41fc9"),
        backup_url: None,
        attribution: Some("SpeakoFlow Mini by SpeakoFlow (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/SpeakoFlow/speakoflow-mini/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: true,
        polish_capable: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Reliable cleanup: restrained, preserves names/numbers, fixes self-corrections",
        runs_on_cpu: true,
        sort_order: 3,
        shards: &[],
    },
    CatalogEntry {
        id: "qwen2.5-3b",
        name: "Qwen2.5 3B",
        size_label: "3B",
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf",
        size_mb: 1900,
        description: "Best local model in our tests. Good for style profiles and transforms.",
        purpose: "",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen2.5 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: true,
        polish_capable: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Best in our tests, may follow dictated instructions",
        runs_on_cpu: false,
        sort_order: 4,
        shards: &[],
    },
    CatalogEntry {
        id: "fast",
        name: "Fast (Qwen3 1.7B)",
        size_label: "1.7B",
        url: "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/qwen3-1.7b-q5_k_m.gguf",
        size_mb: 1400,
        description: "Quick local transforms. Good for Polish and short rewrites.",
        purpose: "",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen3 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        polish_capable: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Fast, good for Polish and short rewrites",
        runs_on_cpu: false,
        sort_order: 5,
        shards: &[],
    },
    CatalogEntry {
        id: "qwen3-0.6b",
        name: "Qwen3 0.6B",
        size_label: "0.6B",
        url: "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q8_0.gguf",
        size_mb: 610,
        description: "Very small and fast. Good for low-RAM Macs.",
        purpose: "",
        sha256: Some("9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031"),
        backup_url: None,
        attribution: Some("Qwen3 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        polish_capable: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Scored well, very small download",
        runs_on_cpu: true,
        sort_order: 6,
        shards: &[],
    },
    CatalogEntry {
        id: "quality",
        name: "Quality (Qwen3 4B)",
        size_label: "4B",
        url: "https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/qwen3-4b-q4_k_m.gguf",
        size_mb: 2600,
        description: "Better quality for Professional and Prompt Engineer transforms.",
        purpose: "",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen3 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen3-4B-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        polish_capable: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Better quality for complex transforms",
        runs_on_cpu: false,
        sort_order: 7,
        shards: &[],
    },
    CatalogEntry {
        id: "qwen2.5-7b",
        name: "Qwen2.5 7B",
        size_label: "7B",
        url: "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m.gguf",
        size_mb: 4900,
        description: "Highest quality local model. Slower on older Macs.",
        purpose: "",
        sha256: None,
        backup_url: None,
        attribution: Some("Qwen2.5 by Alibaba (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/blob/main/LICENSE"),
        requires_license_accept: false,
        recommended: false,
        polish_capable: true,
        verdict: ModelVerdict::Recommended,
        verdict_note: "Resists dictated instructions, sometimes drops words",
        runs_on_cpu: false,
        sort_order: 8,
        shards: &[],
    },
    CatalogEntry {
        id: "gemma2-2b",
        name: "Gemma 2 2B",
        size_label: "2B",
        url: "https://huggingface.co/google/gemma-2-2b-it-GGUF/resolve/main/gemma-2-2b-it-Q4_K_M.gguf",
        size_mb: 1500,
        description: "Google's small model. Mixed results with non-English dictation.",
        purpose: "",
        sha256: None,
        backup_url: None,
        attribution: Some("Gemma 2 by Google (Google License)"),
        license_name: Some("Google Gemma Terms of Use"),
        license_url: Some("https://ai.google.dev/gemma/terms"),
        requires_license_accept: false,
        recommended: false,
        polish_capable: true,
        verdict: ModelVerdict::Mixed,
        verdict_note: "Mixed results, often mishandles other languages",
        runs_on_cpu: false,
        sort_order: 9,
        shards: &[],
    },
    CatalogEntry {
        id: "neudecide-q4",
        name: "NeuDecide Q4 (decider)",
        size_label: "55.5M",
        url: "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/config.json",
        size_mb: 41,
        description: "Speech-to-tool-call decider for Command Mode (ONNX, runs in-process).",
        purpose: "",
        sha256: None,
        backup_url: Some("https://huggingface.co/neuphonic/neudecide/resolve/main/config.json"),
        attribution: Some("NeuDecide by neuphonic (Apache-2.0)"),
        license_name: Some("Apache-2.0"),
        license_url: Some("https://huggingface.co/neuphonic/neudecide/blob/main/README.md"),
        requires_license_accept: false,
        // A decider, not a polish model: the badge here told the user this was
        // a recommended way to clean up text, in a rail whose own hint says
        // "Clean up and rewrite your text". Same defect needle3 had.
        recommended: false,
        polish_capable: false,
        verdict: ModelVerdict::NotTested,
        verdict_note: "Default Command Mode decider (Addendum A); not measured on our corpus yet.",
        runs_on_cpu: true,
        sort_order: 10,
        shards: &[
            CatalogShard {
                file_name: "config.json",
                url: "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/config.json",
                size_bytes: 1_002,
                sha256: "a1eb5783b7843541b193258c1296d616a4f3f9f5893745bac74c5ef19b2e53a0",
            },
            CatalogShard {
                file_name: "audio_encoder_q4.onnx",
                url: "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/audio_encoder_q4.onnx",
                size_bytes: 22_519_972,
                sha256: "2ac5c48e1766382736ed8dd4d468568ee583536550fb741ac187219f8cd8da9b",
            },
            CatalogShard {
                file_name: "tool_encoder_q4.onnx",
                url: "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/tool_encoder_q4.onnx",
                size_bytes: 10_090_217,
                sha256: "4a5075ee2868d90ea4be3234eae04001be6886c9730e8746421fa17e84313d85",
            },
            CatalogShard {
                file_name: "decoder_step_q4.onnx",
                url: "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/decoder_step_q4.onnx",
                size_bytes: 9_951_225,
                sha256: "ee68bbb4bbca552b909b41cb1fdbfdf0f9f3c852561e18036044ee2ff461e50d",
            },
            CatalogShard {
                file_name: "tokenizer.json",
                url: "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/tokenizer.json",
                size_bytes: 228_594,
                sha256: "2b31f2b79506cca36b3c63f0e8549e8401dd8d768175c57aa1e67432d69dee0d",
            },
            CatalogShard {
                file_name: "tokenizer.model",
                url: "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/tokenizer.model",
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
        for id in ["fast", "quality", "s1-mini", "qwen2.5-3b", "neudecide-q4"] {
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

    /// The polish rail takes its badge and its order straight from these two
    /// fields, so flipping either silently un-recommends S1-mini with nothing
    /// else failing.
    #[test]
    fn s1_mini_is_the_recommended_head_of_the_list() {
        let e = find("s1-mini").unwrap();
        assert!(e.recommended, "s1-mini must carry the Recommended badge");
        assert!(
            matches!(e.verdict, ModelVerdict::Recommended),
            "the badge must be backed by a measured verdict"
        );
        assert!(!e.verdict_note.is_empty(), "the badge explains itself");
        let head = CATALOG.iter().min_by_key(|m| m.sort_order).unwrap();
        assert_eq!(head.id, "s1-mini", "the recommended model leads the list");
    }

    #[test]
    fn sort_orders_are_unique() {
        let mut orders: Vec<u32> = CATALOG.iter().map(|e| e.sort_order).collect();
        let n = orders.len();
        orders.sort_unstable();
        orders.dedup();
        assert_eq!(
            orders.len(),
            n,
            "duplicate sort_order makes the list order depend on array position"
        );
    }

    #[test]
    fn badge_requires_a_measured_verdict() {
        // The rail renders the badge from `recommended` alone and nothing else
        // checks the pair, so an untested row could wear it. Two did: needle3
        // ("NOT for polish") and neudecide-q4 (a Command Mode decider), both
        // with verdict NotTested. The rule that would have refused both.
        let bad: Vec<&str> = CATALOG
            .iter()
            .filter(|e| e.recommended && !matches!(e.verdict, ModelVerdict::Recommended))
            .map(|e| e.id)
            .collect();
        assert!(bad.is_empty(), "badged without a measured verdict: {bad:?}");
        let unexplained: Vec<&str> = CATALOG
            .iter()
            .filter(|e| e.recommended && e.verdict_note.is_empty())
            .map(|e| e.id)
            .collect();
        assert!(
            unexplained.is_empty(),
            "the badge has to say why: {unexplained:?}"
        );
    }

    #[test]
    fn only_polish_engines_are_offered_as_polish_models() {
        // The rail is titled "Polish models" and its hint promises cleanup, so
        // a decider must not appear in it as if it were one.
        // `polish_capable` is the gate; this is the only row the app loads
        // for some other job.
        let off: Vec<&str> = CATALOG
            .iter()
            .filter(|e| !e.polish_capable)
            .map(|e| e.id)
            .collect();
        assert_eq!(off, vec!["neudecide-q4"]);
        for e in CATALOG.iter().filter(|e| !e.polish_capable) {
            assert!(!e.recommended, "{} is badged but cannot polish", e.id);
        }
    }

    /// The CPU cutoff is a line on the parameter scale, so the set it withholds
    /// has to be named rather than implied: if a row moves across without anyone
    /// deciding, a Windows user is quietly shown (or hidden from) a model.
    #[test]
    fn runs_on_cpu_withholds_exactly_the_multi_billion_parameter_rows() {
        let heavy: Vec<&str> = CATALOG
            .iter()
            .filter(|e| !e.runs_on_cpu)
            .map(|e| e.id)
            .collect();
        assert_eq!(
            heavy,
            vec![
                "qwen3.5-2b",
                "qwen2.5-3b",
                "fast",
                "quality",
                "qwen2.5-7b",
                "gemma2-2b"
            ],
            "the rows a CPU host is not offered"
        );
        // The kept set stays under the cutoff: 0.8B / 834 MB is the largest.
        for e in CATALOG.iter().filter(|e| e.runs_on_cpu) {
            assert!(
                e.size_mb <= 900,
                "{} is {} MB and claims to run on CPU",
                e.id,
                e.size_mb
            );
        }
    }

    /// A shortlist row must be a polish model this host can actually use, or the
    /// first-run card promises a download that turns into a crawl.
    #[test]
    fn polish_shortlist_rows_are_all_polish_models_this_host_can_run() {
        for &gpu in &[true, false] {
            let ids = polish_shortlist_ids(gpu);
            assert!(!ids.is_empty(), "gpu={gpu} shortlisted nothing");
            assert!(ids.len() <= POLISH_SHORTLIST_MAX);
            let mut seen = std::collections::HashSet::new();
            for id in &ids {
                let e = find(id).unwrap_or_else(|| panic!("{id} is not a catalog entry"));
                assert!(e.polish_capable, "{id} is a decider, not a polish model");
                assert!(e.runs_on_host(gpu), "{id} cannot run on gpu={gpu}");
                assert!(
                    seen.insert(*id),
                    "{id} appears twice in the gpu={gpu} shortlist"
                );
            }
        }
    }

    #[test]
    fn polish_shortlist_leads_with_a_recommended_model_and_matches_the_hosts() {
        assert_eq!(polish_shortlist_ids(true), vec!["s1-mini", "qwen3.5-2b"]);
        assert_eq!(
            polish_shortlist_ids(false),
            vec!["s1-mini", "speakoflow-mini"]
        );
        // The head is the row every host gets first, and it wears the badge, so
        // the card a user cannot skip is a model that was measured.
        for &gpu in &[true, false] {
            let head = find(&polish_shortlist_ids(gpu)[0]).unwrap();
            assert!(head.recommended, "the lead row must be a recommended model");
            assert!(head.runs_on_cpu, "the lead row must run on every host");
        }
    }

    /// The two shortlisted rows land beside each other, so each one has to say
    /// which job it is good at. Without the label the pair reads as a coin
    /// flip, and the cleanup models cannot follow an instruction at all.
    #[test]
    fn shortlisted_rows_name_the_job_they_are_for() {
        assert_eq!(find("s1-mini").unwrap().purpose, "Dictation cleanup");
        assert_eq!(
            find("qwen3.5-2b").unwrap().purpose,
            "Polish + Prompt Engineer"
        );
    }

    /// The 2B is the first row above the CPU cutoff, so its pin and its size
    /// ceiling are the two facts the shortlist depends on.
    #[test]
    fn qwen35_is_pinned_and_stays_under_the_size_ceiling() {
        let e = find("qwen3.5-2b").unwrap();
        assert!(e.size_mb <= 1200, "the user's ceiling is 1200 MB");
        assert!(
            e.sha256.is_some(),
            "an unpinned download can change content silently"
        );
        assert!(e.url.ends_with("Qwen3.5-2B-IQ4_XS.gguf"));
        assert!(!e.runs_on_cpu, "a 2B at 1.1 GB stays off a CPU-only host");
    }

    /// Apple Silicon is the only build with a fast Metal backend today, and the
    /// answer must be the one this test host agrees with, not a compile-time
    /// guess the command layer contradicts.
    #[test]
    fn host_gpu_accelerated_follows_the_detected_platform() {
        let expected = matches!(
            teletype_speech::catalog::host_platform(),
            teletype_speech::catalog::Platform::MacosAppleSilicon { .. }
        );
        assert_eq!(host_gpu_accelerated(), expected);
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
            assert!(s.url.starts_with(
                "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/"
            ));
            assert!(s.size_bytes > 0);
        }
        // A.3.1: the re-host (GitHub release) is primary; upstream HF — gated
        // behind a contact-information form — stays as the backup URL.
        assert!(e.url.starts_with(
            "https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/"
        ));
        assert_eq!(
            e.backup_url,
            Some("https://huggingface.co/neuphonic/neudecide/resolve/main/config.json")
        );
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
            purpose: "",
            sha256: None,
            backup_url: None,
            attribution: None,
            license_name: None,
            license_url: None,
            requires_license_accept: false,
            recommended: false,
            polish_capable: true,
            runs_on_cpu: true,
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
