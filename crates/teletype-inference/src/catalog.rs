//! Built-in model catalog.
//!
//! V1 ships two Qwen3-class GGUF models. Users can also point to any local
//! GGUF file (BYOM) — the catalog is a convenience, not a requirement.

use serde::{Deserialize, Serialize};

/// One catalog entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    /// Approximate parameter class, for display.
    pub size_label: &'static str,
    /// Hugging Face URL for the GGUF file.
    pub url: &'static str,
    /// Approximate download size in MB.
    pub size_mb: u32,
    pub description: &'static str,
}

/// The built-in catalog.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "fast",
        name: "Fast (Qwen3 1.7B)",
        size_label: "1.7B",
        url: "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/qwen3-1.7b-q5_k_m.gguf",
        size_mb: 1400,
        description: "Quick local transforms. Good for Polish and short rewrites.",
    },
    CatalogEntry {
        id: "quality",
        name: "Quality (Qwen3 4B)",
        size_label: "4B",
        url: "https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/qwen3-4b-q4_k_m.gguf",
        size_mb: 2600,
        description: "Better quality for Professional and Prompt Engineer transforms.",
    },
];

/// Finds a catalog entry by id.
pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}
