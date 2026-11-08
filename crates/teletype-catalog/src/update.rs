//! Silent-update path: fetch a release manifest, verify file hashes,
//! validate the catalog, and swap the working copy only after a full pass.
//!
//! R11 (locked, shared with Task 12): `Manifest.files` keys are EXACTLY
//! `speech`, `llm`, `command` (each the sha256 of that .json body); each file
//! is fetched from `{url}/{key}.json`. `Manifest.version` is a monotonic u64
//! supplied by CI via CATALOG_VERSION, never parsed from a semver tag. The
//! installed version is a u64 text file at `working_copy_dir/version`
//! (absent => 0).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::schema::Catalog;

/// Throttle window: at most one check per hour.
const CHECK_INTERVAL: Duration = Duration::from_secs(3600);

const CATALOG_FILES: [&str; 3] = ["speech", "llm", "command"];

/// Fixed manifest URL (R11): the release asset `catalog-manifest.json`.
pub const MANIFEST_URL: &str = "https://teletype-catalog/manifest/catalog-manifest.json";

/// Release manifest (`catalog-manifest.json`), R11 contract.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    /// Monotonic u64 supplied by CI (CATALOG_VERSION). Not a semver tag.
    pub version: u64,
    /// Exactly the keys `speech`, `llm`, `command`; each value is the sha256
    /// (64 lowercase hex) of the corresponding `.json` body.
    pub files: HashMap<String, String>,
    /// Base URL; each file is fetched from `{url}/{key}.json`.
    pub url: String,
}

/// Outcome of a `try_update` call.
#[derive(Debug, PartialEq, Eq)]
pub enum UpdateResult {
    /// A newer, verified, validated catalog replaced the working copy.
    Updated,
    /// The manifest version is not greater than the installed version.
    Current,
    /// Throttled: a check happened within the last hour.
    Skipped,
    /// Fetch, hash, or validation failed; the last good copy is untouched.
    Failed,
}

/// HTTP abstraction so tests can stub the network.
pub trait Fetch {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// The working-copy directory: `data_dir/catalogs`.
pub fn working_copy_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("catalogs")
}

/// Where the throttle timestamp is persisted.
pub fn last_check_path(data_dir: &Path) -> PathBuf {
    data_dir.join("catalog_last_check")
}

/// True iff `last_check` is None or at least an hour has elapsed.
pub fn should_check(last_check: Option<SystemTime>, now: SystemTime) -> bool {
    match last_check {
        None => true,
        Some(t) => now.duration_since(t).is_ok_and(|d| d >= CHECK_INTERVAL),
    }
}

/// Copy the three bundled JSON files into `dir` if absent. Called by startup
/// (Task 12) so a first launch has a working copy to load.
pub fn seed_from_bundled(dir: &Path) {
    std::fs::create_dir_all(dir).expect("create working copy dir");
    for key in CATALOG_FILES {
        let dest = dir.join(format!("{key}.json"));
        if dest.exists() {
            continue;
        }
        let bundled = match key {
            "speech" => include_str!("../speech.json"),
            "llm" => include_str!("../llm.json"),
            "command" => include_str!("../command.json"),
            _ => unreachable!("catalog file key"),
        };
        std::fs::write(&dest, bundled).expect("write bundled catalog file");
    }
}

fn read_installed_version(data_dir: &Path) -> u64 {
    let p = working_copy_dir(data_dir).join("version");
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn read_last_check(data_dir: &Path) -> Option<SystemTime> {
    let raw = std::fs::read_to_string(last_check_path(data_dir)).ok()?;
    let secs: u64 = raw.trim().parse().ok()?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs))
}

/// Persist the throttle timestamp (epoch seconds).
pub fn write_last_check(data_dir: &Path, now: SystemTime) {
    let secs = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = std::fs::write(last_check_path(data_dir), secs.to_string());
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// The seven-step silent-update flow. Any failure keeps the last good
/// working copy; a bad published file cannot take the app down.
pub fn try_update(data_dir: &Path, fetch: &dyn Fetch) -> UpdateResult {
    let now = SystemTime::now();

    // 1. Throttle.
    let last_check = read_last_check(data_dir);
    if !should_check(last_check, now) {
        return UpdateResult::Skipped;
    }

    // 2. Fetch manifest. The manifest URL is a fixed contract (R11): the
    //    release asset `catalog-manifest.json`; tests stub it by suffix.
    let manifest_bytes = match fetch.get(MANIFEST_URL) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("catalog update: manifest fetch failed: {e}");
            return UpdateResult::Failed;
        }
    };
    let manifest: Manifest = match serde_json::from_slice(&manifest_bytes) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("catalog update: bad manifest JSON: {e}");
            return UpdateResult::Failed;
        }
    };
    // Sanity: R11 requires exactly the keys speech, llm, command.
    if manifest.files.len() != 3
        || !manifest
            .files
            .keys()
            .all(|k| CATALOG_FILES.contains(&k.as_str()))
    {
        tracing::warn!("catalog update: manifest files keys violate R11");
        return UpdateResult::Failed;
    }

    // 3. Compare version.
    let installed = read_installed_version(data_dir);
    if manifest.version <= installed {
        return UpdateResult::Current;
    }

    let wc = working_copy_dir(data_dir);
    if std::fs::create_dir_all(&wc).is_err() {
        return UpdateResult::Failed;
    }

    // 4. Download changed files to temp; verify sha256.
    //    key -> (temp path, verified body)
    let mut staged: Vec<(&str, PathBuf, Vec<u8>)> = Vec::new();
    for key in CATALOG_FILES {
        let filename = format!("{key}.json");
        let expected = manifest.files.get(key).map(|s| s.as_str());
        if let Some(expected) = expected {
            // Unchanged file: keep the working copy as-is, skip download.
            let current = std::fs::read(wc.join(&filename));
            if let Ok(body) = &current {
                if sha256_hex(body) == expected {
                    continue;
                }
            }
            let url = format!("{}/{}", manifest.url, filename);
            let body = fetch.get(&url);
            match body {
                Ok(body) => {
                    if sha256_hex(&body) != expected {
                        tracing::warn!("catalog update: sha256 mismatch for {filename}");
                        return UpdateResult::Failed;
                    }
                    // Stage to a temp file in the same dir for the atomic rename.
                    let tmp = wc.join(format!("{filename}.tmp"));
                    if std::fs::write(&tmp, &body).is_err() {
                        return UpdateResult::Failed;
                    }
                    staged.push((key, tmp, body));
                }
                Err(e) => {
                    tracing::warn!("catalog update: fetch {url} failed: {e}");
                    return UpdateResult::Failed;
                }
            }
        } else if !wc.join(&filename).exists() {
            // Missing key and no working copy to keep: cannot proceed.
            tracing::warn!("catalog update: manifest missing key {key}");
            return UpdateResult::Failed;
        }
    }

    // 5. Parse all three into a Catalog and validate. Staged files are read
    //    from their temp paths; unchanged files from the working copy.
    let read_staged = |key: &str| -> Result<Vec<u8>, std::io::Error> {
        for (k, _tmp, body) in &staged {
            if *k == key {
                return Ok(body.clone());
            }
        }
        std::fs::read(wc.join(format!("{key}.json")))
    };
    let result = (|| -> Result<Catalog, String> {
        let parse = |key: &str, name: &str| -> Result<serde_json::Value, String> {
            let body = read_staged(key).map_err(|e| e.to_string())?;
            serde_json::from_slice::<Vec<serde_json::Value>>(&body)
                .map(serde_json::Value::Array)
                .map_err(|e| format!("{name}: {e}"))
        };
        let speech: Vec<crate::schema::SpeechEntry> =
            serde_json::from_value(parse("speech", "speech.json")?).map_err(|e| e.to_string())?;
        let llm: Vec<crate::schema::LlmEntry> =
            serde_json::from_value(parse("llm", "llm.json")?).map_err(|e| e.to_string())?;
        let command: Vec<crate::schema::CommandEntry> =
            serde_json::from_value(parse("command", "command.json")?).map_err(|e| e.to_string())?;
        let catalog = Catalog {
            speech,
            llm,
            command,
        };
        crate::validate::validate_catalog(&catalog).map_err(|e| e.to_string())?;
        Ok(catalog)
    })();
    if let Err(e) = &result {
        tracing::warn!("catalog update: validation failed: {e}");
        for (_, tmp, _) in &staged {
            let _ = std::fs::remove_file(tmp);
        }
        return UpdateResult::Failed;
    }

    // 6. Atomic swap: rename each staged temp over the working copy, bump the
    //    version, write the throttle timestamp.
    for (key, tmp, _) in &staged {
        if std::fs::rename(tmp, wc.join(format!("{key}.json"))).is_err() {
            return UpdateResult::Failed;
        }
    }
    if std::fs::write(wc.join("version"), manifest.version.to_string()).is_err() {
        return UpdateResult::Failed;
    }
    write_last_check(data_dir, now);
    UpdateResult::Updated
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemFetch {
        files: HashMap<String, Vec<u8>>,
    }

    impl Fetch for MemFetch {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            self.files
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no {url}"))
        }
    }

    #[test]
    fn swap_renames_staged_files_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let wc = working_copy_dir(dir.path());
        std::fs::create_dir_all(&wc).unwrap();
        // Seeded from the real bundled files (valid catalog), llm staged.
        seed_from_bundled(&wc);
        let speech = std::fs::read_to_string(wc.join("speech.json")).unwrap();
        let command = std::fs::read_to_string(wc.join("command.json")).unwrap();
        let llm = good_llm_body();
        let manifest: Manifest = serde_json::from_str(&format!(
            r#"{{ "version": 7, "files": {{ "speech": "{}", "llm": "{}", "command": "{}" }}, "url": "https://x.example/base" }}"#,
            sha256_hex(speech.as_bytes()),
            sha256_hex(llm.as_bytes()),
            sha256_hex(command.as_bytes()),
        ))
        .unwrap();
        // Stub: manifest + only the changed llm file.
        let mut files = HashMap::new();
        files.insert(
            MANIFEST_URL.to_string(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        files.insert(
            format!("{}/llm.json", manifest.url),
            llm.as_bytes().to_vec(),
        );
        let res = try_update(dir.path(), &MemFetch { files });
        assert!(matches!(res, UpdateResult::Updated));
        assert_eq!(std::fs::read_to_string(wc.join("llm.json")).unwrap(), llm);
        assert_eq!(
            std::fs::read_to_string(wc.join("speech.json")).unwrap(),
            speech
        );
        assert_eq!(std::fs::read_to_string(wc.join("version")).unwrap(), "7");
        // No temp files left behind.
        for key in CATALOG_FILES {
            assert!(!wc.join(format!("{key}.json.tmp")).exists());
        }
    }

    fn good_llm_body() -> String {
        include_str!("../llm.json").to_string()
    }
}
