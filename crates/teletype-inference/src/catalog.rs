//! Built-in LLM model catalog.
//!
//! The rows live in `teletype-catalog`'s bundled `llm.json` (Phase A); this
//! module wraps each parsed [`LlmEntry`] in a [`CatalogEntry`] that carries
//! the disk predicates and path helpers the app, the downloader, and the
//! first-run shortlist read. The Command Mode decider lives in
//! `command.json` (a `CommandEntry`, not an `LlmEntry`), so it is absent from
//! `find()` and from the polish list.

use std::path::{Path, PathBuf};

use teletype_catalog::schema::{Artifact, Capability, LlmEntry};
use teletype_catalog::Catalog;

/// A file exists and is not obviously truncated.
///
/// See [`CatalogEntry::is_downloaded`] for why this checks size and not the
/// hash.
fn has_plausible_size(path: &Path, expected_bytes: u64) -> bool {
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

/// Measured quality verdict for a model, based on benchmark results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// The `llm.json` string for this verdict.
    pub fn as_data(self) -> &'static str {
        match self {
            Self::FirstParty => "first_party",
            Self::Recommended => "recommended",
            Self::Mixed => "mixed",
            Self::Unreliable => "unreliable",
            Self::NotRecommended => "not_recommended",
            Self::NotTested => "not_tested",
        }
    }

    /// Parse the `llm.json` verdict string.
    pub fn from_data(s: &str) -> Option<Self> {
        match s {
            "first_party" => Some(Self::FirstParty),
            "recommended" => Some(Self::Recommended),
            "mixed" => Some(Self::Mixed),
            "unreliable" => Some(Self::Unreliable),
            "not_recommended" => Some(Self::NotRecommended),
            "not_tested" => Some(Self::NotTested),
            _ => None,
        }
    }
}

/// One shard of a multi-file (split) GGUF model.
#[derive(Debug, Clone)]
pub struct CatalogShard {
    pub file_name: String,
    pub url: String,
    pub size_bytes: u64,
    /// Lowercase hex SHA-256 of the full file.
    pub sha256: String,
}

impl From<&Artifact> for CatalogShard {
    fn from(a: &Artifact) -> Self {
        Self {
            file_name: a.file.clone(),
            url: String::new(),
            size_bytes: a.bytes,
            sha256: a.sha256.clone().unwrap_or_default(),
        }
    }
}

/// One catalog entry: a parsed [`LlmEntry`] plus the disk predicates and path
/// helpers the consumers read.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    entry: LlmEntry,
    shards: Vec<CatalogShard>,
}

impl CatalogEntry {
    pub fn from_entry(entry: LlmEntry) -> Self {
        let shards = entry.shards.iter().map(CatalogShard::from).collect();
        Self { entry, shards }
    }

    /// The parsed entry (url, license, capabilities, ...).
    pub fn entry(&self) -> &LlmEntry {
        &self.entry
    }

    pub fn id(&self) -> &str {
        &self.entry.id
    }

    pub fn name(&self) -> &str {
        &self.entry.name
    }

    /// Approximate parameter class, for display.
    pub fn size_label(&self) -> &str {
        &self.entry.size_label
    }

    /// Primary download URL.
    pub fn url(&self) -> &str {
        &self.entry.url
    }

    /// Approximate download size in MB.
    pub fn size_mb(&self) -> u32 {
        self.entry.size_mb
    }

    pub fn description(&self) -> &str {
        &self.entry.description
    }

    /// The one-phrase job this row is good at, shown as a chip in the Models
    /// rail.
    pub fn specialty(&self) -> &str {
        &self.entry.specialty
    }

    /// Lowercase hex SHA-256 for single-file models; `None` when unknown.
    pub fn sha256(&self) -> Option<&str> {
        self.entry.sha256.as_deref()
    }

    /// Alternate URL tried when the primary fails (single-file only).
    pub fn backup_url(&self) -> Option<&str> {
        self.entry.backup_url.as_deref()
    }

    /// Attribution string shown in the UI, e.g. "S1-mini by Superwhisper".
    pub fn attribution(&self) -> Option<&str> {
        self.entry.attribution.as_deref()
    }

    /// SPDX-style license label, e.g. "Apache-2.0".
    pub fn license_name(&self) -> Option<&str> {
        self.entry.license_name.as_deref()
    }

    /// Deep link to the full license text.
    pub fn license_url(&self) -> Option<&str> {
        self.entry.license_url.as_deref()
    }

    /// When true the UI must show an accept checkbox before downloading.
    pub fn requires_license_accept(&self) -> bool {
        self.entry.requires_license_accept
    }

    /// When true the UI shows a "Recommended" badge next to the model.
    pub fn recommended(&self) -> bool {
        self.entry.recommended
    }

    /// False for models the app can never load as a polish or transform
    /// engine. The LLM domain only carries polish rows, so this is the
    /// `polish` capability check that replaced the old static bool.
    pub fn polish_capable(&self) -> bool {
        self.entry.capabilities.contains(&Capability::Polish)
    }

    /// True when a host with no GPU-accelerated llama-server should be shown
    /// this model as a workable choice.
    pub fn runs_on_cpu(&self) -> bool {
        self.entry.runs_on_cpu
    }

    /// Measured quality verdict from benchmark runs. Unknown strings fall
    /// back to [`ModelVerdict::NotTested`]: a verdict the app cannot name is
    /// no stronger than "not tested", and a row is never hidden for it.
    pub fn verdict(&self) -> ModelVerdict {
        ModelVerdict::from_data(&self.entry.verdict).unwrap_or(ModelVerdict::NotTested)
    }

    /// One-line note explaining the verdict.
    pub fn verdict_note(&self) -> &str {
        &self.entry.verdict_note
    }

    /// Display priority in the model list (lower = higher on the list).
    pub fn sort_order(&self) -> u32 {
        self.entry.sort_order
    }

    /// Non-empty for multi-file (split) GGUFs. Install dir is `models/<id>/`
    /// and the llama-server entrypoint is the first shard.
    pub fn shards(&self) -> &[CatalogShard] {
        &self.shards
    }

    /// True when the model is fully present on disk under `models_dir`.
    ///
    /// Size is checked rather than the SHA-256, because hashing every shard on
    /// every call would be several seconds of disk I/O on a screen refresh. The
    /// download path already verifies the hash before the file is moved into
    /// place, so by the time a file is here it is either correct or the result
    /// of a download that was interrupted, and a truncated file is short.
    pub fn is_downloaded(&self, models_dir: &Path) -> bool {
        if self.shards.is_empty() {
            // `size_mb` is rounded to the nearest MB, so allow 5% either way.
            let expected = u64::from(self.size_mb()).saturating_mul(1_000_000);
            has_plausible_size(&models_dir.join(format!("{}.gguf", self.id())), expected)
        } else {
            self.shards
                .iter()
                .all(|s| has_plausible_size(&models_dir.join(self.id()).join(&s.file_name), s.size_bytes))
        }
    }

    /// Path llama-server should be pointed at (`-m`).
    pub fn entrypoint(&self, models_dir: &Path) -> PathBuf {
        if self.shards.is_empty() {
            models_dir.join(format!("{}.gguf", self.id()))
        } else {
            models_dir.join(self.id()).join(&self.shards[0].file_name)
        }
    }

    /// True when this host can work with the model: see
    /// [`CatalogEntry::runs_on_cpu`] for what the flag means and what it does
    /// not claim. Used for the first-run shortlist and the "this computer"
    /// grouping, never to hide a row from Settings, where a user with a 32 GB
    /// Windows box is entitled to pick the 7B model and wait.
    pub fn runs_on_host(&self, gpu_accelerated: bool) -> bool {
        gpu_accelerated || self.runs_on_cpu()
    }
}

/// The parsed LLM rows, one owned [`CatalogEntry`] per `llm.json` row.
pub fn catalog() -> Vec<CatalogEntry> {
    Catalog::bundled()
        .llm()
        .iter()
        .cloned()
        .map(CatalogEntry::from_entry)
        .collect()
}

/// Finds a catalog entry by id.
pub fn find(id: &str) -> Option<CatalogEntry> {
    catalog().into_iter().find(|e| e.id() == id)
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
pub fn polish_shortlist_ids(gpu_accelerated: bool) -> Vec<String> {
    const CPU: &[&str] = &["s1-mini", "speakoflow-mini"];
    const ACCELERATED: &[&str] = &["s1-mini", "qwen3.5-2b"];

    let ids = if gpu_accelerated { ACCELERATED } else { CPU };
    ids.iter()
        .copied()
        .filter(|id| find(id).is_some_and(|e| e.polish_capable() && e.runs_on_host(gpu_accelerated)))
        .take(POLISH_SHORTLIST_MAX)
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_known_ids() {
        for id in ["fast", "quality", "s1-mini", "qwen2.5-3b"] {
            assert!(find(id).is_some(), "missing {id}");
        }
        // The decider lives in command.json, not the LLM domain.
        assert!(find("neudecide-q4").is_none());
        assert!(find("nope").is_none());
    }

    #[test]
    fn llm_find_is_backed_by_the_parsed_catalog() {
        let parsed = Catalog::bundled();
        assert!(find("s1-mini").is_some());
        assert!(parsed.llm_by_id("s1-mini").is_some());
        assert!(parsed.llm_by_id("neudecide-q4").is_none());
    }

    #[test]
    fn s1_mini_has_checksum_and_no_license_gate() {
        let e = find("s1-mini").unwrap();
        assert!(e.sha256().is_some());
        assert!(!e.requires_license_accept());
        assert!(e.shards().is_empty());
        assert_eq!(e.attribution(), Some("S1-mini by Superwhisper"));
    }

    /// The polish rail takes its badge and its order straight from these two
    /// fields, so flipping either silently un-recommends S1-mini with nothing
    /// else failing.
    #[test]
    fn s1_mini_is_the_recommended_head_of_the_list() {
        let e = find("s1-mini").unwrap();
        assert!(e.recommended(), "s1-mini must carry the Recommended badge");
        assert!(
            matches!(e.verdict(), ModelVerdict::Recommended),
            "the badge must be backed by a measured verdict"
        );
        assert!(!e.verdict_note().is_empty(), "the badge explains itself");
        let head = catalog().into_iter().min_by_key(|m| m.sort_order()).unwrap();
        assert_eq!(head.id(), "s1-mini", "the recommended model leads the list");
    }

    #[test]
    fn sort_orders_are_unique() {
        let mut orders: Vec<u32> = catalog().iter().map(|e| e.sort_order()).collect();
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
        // checks the pair, so an untested row could wear it. The rule that
        // would have refused them.
        let rows = catalog();
        let bad: Vec<&str> = rows
            .iter()
            .filter(|e| e.recommended() && !matches!(e.verdict(), ModelVerdict::Recommended))
            .map(|e| e.id())
            .collect();
        assert!(bad.is_empty(), "badged without a measured verdict: {bad:?}");
        let unexplained: Vec<&str> = rows
            .iter()
            .filter(|e| e.recommended() && e.verdict_note().is_empty())
            .map(|e| e.id())
            .collect();
        assert!(
            unexplained.is_empty(),
            "the badge has to say why: {unexplained:?}"
        );
    }

    /// The LLM domain is the polish domain: every row polishes, and the decider
    /// (a `CommandEntry` in command.json) is not here at all.
    #[test]
    fn only_polish_engines_are_offered_as_polish_models() {
        let rows = catalog();
        assert!(!rows.is_empty(), "the LLM domain must not be empty");
        let off: Vec<&str> = rows
            .iter()
            .filter(|e| !e.polish_capable())
            .map(|e| e.id())
            .collect();
        assert!(
            off.is_empty(),
            "a non-polish row sat in the LLM domain: {off:?}"
        );
        assert!(find("neudecide-q4").is_none(), "the decider is not an LLM row");
    }

    /// The CPU cutoff is a line on the parameter scale, so the set it withholds
    /// has to be named rather than implied: if a row moves across without anyone
    /// deciding, a Windows user is quietly shown (or hidden from) a model.
    #[test]
    fn runs_on_cpu_withholds_exactly_the_multi_billion_parameter_rows() {
        let rows = catalog();
        let heavy: Vec<&str> = rows
            .iter()
            .filter(|e| !e.runs_on_cpu())
            .map(|e| e.id())
            .collect();
        let mut heavy = heavy;
        heavy.sort();
        assert_eq!(
            heavy,
            vec!["fast", "gemma2-2b", "quality", "qwen2.5-3b", "qwen2.5-7b", "qwen3.5-2b"],
            "the rows a CPU host is not offered"
        );
        // The kept set stays under the cutoff: 0.8B / 834 MB is the largest.
        for e in rows.iter().filter(|e| e.runs_on_cpu()) {
            assert!(
                e.size_mb() <= 900,
                "{} is {} MB and claims to run on CPU",
                e.id(),
                e.size_mb()
            );
        }
    }

    /// A shortlist row must be a polish model this host can actually use, or the
    /// first-run card promises a download that turns into a crawl.
    #[test]
    fn polish_shortlist_rows_are_all_polish_models_this_host_can_run() {
        for gpu in [true, false] {
            let ids = polish_shortlist_ids(gpu);
            assert!(!ids.is_empty(), "gpu={gpu} shortlisted nothing");
            assert!(ids.len() <= POLISH_SHORTLIST_MAX);
            let mut seen = std::collections::HashSet::new();
            for id in &ids {
                let e = find(id).unwrap_or_else(|| panic!("{id} is not a catalog entry"));
                assert!(e.polish_capable(), "{id} is a decider, not a polish model");
                assert!(e.runs_on_host(gpu), "{id} cannot run on gpu={gpu}");
                assert!(
                    seen.insert(id.clone()),
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
        for gpu in [true, false] {
            let head = find(&polish_shortlist_ids(gpu)[0]).unwrap();
            assert!(head.recommended(), "the lead row must be a recommended model");
            assert!(head.runs_on_cpu(), "the lead row must run on every host");
        }
    }

    /// The two shortlisted rows land beside each other, so each one has to say
    /// which job it is good at. Without the label the pair reads as a coin
    /// flip, and the cleanup models cannot follow an instruction at all.
    #[test]
    fn shortlisted_rows_name_the_job_they_are_for() {
        let s1 = find("s1-mini").unwrap();
        assert_eq!(s1.specialty(), "cleanup");
        let qwen35 = find("qwen3.5-2b").unwrap();
        assert_eq!(qwen35.specialty(), "all_round");
    }

    /// The measured specialties are the rows this repo's bakeoff proved, pinned
    /// per row so a data edit that promotes an inferred role to a measured one
    /// fails here instead of shipping a chip that reads as a score.
    #[test]
    fn measured_specialties_are_exactly_the_bakeoff_rows() {
        let rows = catalog();
        let measured: Vec<&str> = rows
            .iter()
            .filter(|e| e.polish_capable() && matches!(e.specialty(), "cleanup" | "all_round"))
            .map(|e| e.id())
            .collect();
        assert_eq!(
            measured,
            vec!["s1-mini", "qwen3.5-2b", "speakoflow-mini"],
            "a measured specialty is only true for a role this repo's bakeoff proved"
        );
    }

    /// The rail sorts by `sort_order`, and a row the user is steering to by
    /// chip should not rank above a measured one: "Small & fast" without a
    /// measured role sinks below the bakeoff-proven rows, or the chip list
    /// reads as if every model was scored.
    #[test]
    fn unmeasured_rows_rank_below_measured_rows() {
        let rows = catalog();
        // Measured polish rows keep their relative order and lead the list.
        let measured_ids: Vec<&str> = rows
            .iter()
            .filter(|e| e.polish_capable() && matches!(e.specialty(), "cleanup" | "all_round"))
            .map(|e| e.id())
            .collect();
        let measured_ranks: Vec<u32> = measured_ids
            .iter()
            .map(|id| find(id).unwrap().sort_order())
            .collect();
        let mut sorted = measured_ranks.clone();
        sorted.sort();
        assert_eq!(
            measured_ranks, sorted,
            "measured rows are listed in rank order: {measured_ranks:?}"
        );
        let max_measured = *measured_ranks.iter().max().unwrap();

        // Every unmeasured polish row ranks strictly below the measured block.
        let unmeasured: Vec<(&str, u32)> = rows
            .iter()
            .filter(|e| e.polish_capable() && !matches!(e.specialty(), "cleanup" | "all_round"))
            .map(|e| (e.id(), e.sort_order()))
            .collect();
        for (id, rank) in &unmeasured {
            assert!(
                *rank > max_measured,
                "unmeasured {id} (rank {rank}) must sink below the measured block (max {max_measured})"
            );
        }
        assert!(
            !unmeasured.is_empty(),
            "the list must have unmeasured rows to check"
        );

        // Within the unmeasured block the rows keep size order: the 0.6B row
        // leads, then the 1.7B "fast", the 2B Gemma, then the larger instruct
        // rows (3B < 7B), so the chip list reads as a size ladder, not a
        // random permutation.
        let rank = |id: &str| find(id).unwrap().sort_order();
        assert!(rank("qwen3-0.6b") < rank("fast"));
        assert!(rank("fast") < rank("gemma2-2b"));
        assert!(rank("gemma2-2b") < rank("qwen2.5-3b"));
        assert!(rank("qwen2.5-3b") < rank("quality"));
        assert!(rank("quality") < rank("qwen2.5-7b"));

        // Qwen3 0.6B has no measured specialty, so it sinks below the whole
        // measured block even though it is the smallest polish row: the chip
        // list does not pretend it was scored.
        assert!(rank("qwen3-0.6b") > max_measured);
    }

    /// The 2B is the first row above the CPU cutoff, so its pin and its size
    /// ceiling are the two facts the shortlist depends on.
    #[test]
    fn qwen35_is_pinned_and_stays_under_the_size_ceiling() {
        let e = find("qwen3.5-2b").unwrap();
        assert!(e.size_mb() <= 1200, "the user's ceiling is 1200 MB");
        assert!(
            e.sha256().is_some(),
            "an unpinned download can change content silently"
        );
        assert!(e.url().ends_with("Qwen3.5-2B-IQ4_XS.gguf"));
        assert!(!e.runs_on_cpu(), "a 2B at 1.1 GB stays off a CPU-only host");
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

    /// The decider row keeps its six pinned files in command.json, and the
    /// loader reaches them through the command accessor (R3).
    #[test]
    fn neudecide_q4_pins_all_six_files() {
        let parsed = Catalog::bundled();
        let d = parsed.command_by_id("neudecide-q4").expect("the decider is in command.json");
        // Six pinned artifacts: config + 3 ONNX graphs + 2 tokenizers.
        assert_eq!(d.shards.len(), 6);
        for s in &d.shards {
            assert_eq!(
                s.sha256.as_deref().map(str::len).unwrap_or(0),
                64,
                "sha256 of {} must be 64 hex chars",
                s.file
            );
            assert!(s.bytes > 0);
        }
        // A.3.1: the re-host (GitHub release) is primary; upstream HF, gated
        // behind a contact-information form, stays as the backup URL.
        assert!(d
            .base_url
            .as_deref()
            .is_some_and(|b| b.starts_with("https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/")));
        assert_eq!(
            d.backup_url.as_deref(),
            Some("https://huggingface.co/neuphonic/neudecide/resolve/main/config.json")
        );
        // Pinned total: 6 shards = 42,915,902 bytes (40.9 MiB); size_mb
        // rounds the MiB display (same convention as s1-mini: 461.78 to 462).
        let total: u64 = d.shards.iter().map(|s| s.bytes).sum();
        assert_eq!(total, 42_915_902, "neudecide-q4 total byte size drifted");
        // 43 is the decimal-MB display of the pinned total (42,915,902 bytes
        // is 42.9 MB); the row's size_mb rounds that up, not the MiB.
        assert_eq!(d.size_mb, 43);
        // R3: the decider row carries the full display field set the UI reads.
        assert!(!d.size_label.is_empty());
        assert!(!d.specialty.is_empty());
        assert!(!d.description.is_empty());
        assert!(!d.verdict_note.is_empty());
        assert!(d.attribution.is_some());
        assert!(d.license_name.is_some());
        assert!(d.license_url.is_some());
        assert!(!d.recommended);
        // The decider is never a polish row.
        assert!(!d.capabilities.contains(&Capability::Polish));
        assert!(d.capabilities.contains(&Capability::ToolCalling));
        // It is not an LLM row either.
        assert!(parsed.llm_by_id("neudecide-q4").is_none());
    }

    /// A synthetic two-shard entry: exercises the multi-shard install path
    /// without depending on any catalog model.
    fn multi_shard_entry() -> CatalogEntry {
        CatalogEntry::from_entry(LlmEntry {
            id: "test-multi".into(),
            name: "Test multi-shard".into(),
            size_label: "t".into(),
            capabilities: vec![Capability::Polish],
            url: "https://example.invalid/test-multi".into(),
            size_mb: 0,
            description: "synthetic multi-shard entry for tests".into(),
            specialty: "fast_small".into(),
            sha256: None,
            backup_url: None,
            attribution: None,
            license_name: None,
            license_url: None,
            requires_license_accept: false,
            recommended: false,
            runs_on_cpu: true,
            verdict: "not_tested".into(),
            verdict_note: String::new(),
            sort_order: 0,
            shards: vec![
                Artifact {
                    file: "part-00001-of-00002.gguf".into(),
                    bytes: 8,
                    sha256: Some("0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f".into()),
                },
                Artifact {
                    file: "part-00002-of-00002.gguf".into(),
                    bytes: 4,
                    sha256: Some("1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f".into()),
                },
            ],
            runs_on: vec!["macos".into()],
        })
    }

    #[test]
    fn entrypoint_paths_for_single_and_multi() {
        let single = find("s1-mini").unwrap();
        let multi = multi_shard_entry();
        let dir = Path::new("/models");
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
    /// This used to be `is_file()`, so a zero-byte GGUF from an interrupted
    /// download counted as installed. The model screen then showed it as
    /// downloaded, selecting it produced a model that could not load, and the
    /// only symptom was "no model loaded" with nothing pointing at the cause.
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
        let tenth = u64::from(single.size_mb()) * 100_000;
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
        let shard_dir = dir.join(multi.id());
        std::fs::create_dir_all(&shard_dir).unwrap();

        // No shards at all.
        assert!(!multi.is_downloaded(&dir));

        // All shards present but each 0 bytes.
        for s in multi.shards() {
            std::fs::write(shard_dir.join(&s.file_name), b"").unwrap();
        }
        assert!(!multi.is_downloaded(&dir), "0-byte shards are not a model");

        // Every shard at its exact catalog size: now it counts.
        for s in multi.shards() {
            std::fs::write(
                shard_dir.join(&s.file_name),
                vec![0u8; s.size_bytes as usize],
            )
            .unwrap();
        }
        assert!(multi.is_downloaded(&dir));

        // Drop one shard again.
        std::fs::remove_file(shard_dir.join(&multi.shards()[1].file_name)).unwrap();
        assert!(
            !multi.is_downloaded(&dir),
            "one missing shard is not enough"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
