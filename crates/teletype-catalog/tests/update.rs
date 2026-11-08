// Task 11: silent-update path tests. StubFetch replaces HTTP so no real
// network is touched. The four scenarios: newer updates, wrong sha256 keeps
// old, schema error keeps old, recent check skipped.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};
use teletype_catalog::update::*;

const BASE_URL: &str = "https://example.invalid/teletype/releases/download/catalog-v2";

struct StubFetch {
    manifest: String,
    files: HashMap<String, String>,
}

impl Fetch for StubFetch {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        if url.ends_with("catalog-manifest.json") {
            return Ok(self.manifest.as_bytes().to_vec());
        }
        let name = url.rsplit('/').next().unwrap();
        self.files
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing {name}"))
            .map(|s| s.into_bytes())
    }
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

/// Minimal valid catalogs: one speech row (whisper, recommended), one llm row
/// (recommended), one command row (tool_calling). Field sets match the
/// schema exactly (serde has no defaults).
fn good_speech() -> String {
    r#"[
      {
        "id": "tiny-asr",
        "name": "Tiny ASR",
        "engine": "whisper",
        "capabilities": ["asr"],
        "languages": ["en"],
        "englishOnly": true,
        "artifacts": [
          {
            "file": "tiny.bin",
            "bytes": 1000,
            "sha256": "4d64e9e96c2792186d072fde0034df0ad670cf680a2f53069052ead827fd600e"
          }
        ],
        "baseUrl": null,
        "sizeMb": 1,
        "minRamGb": 1,
        "speed": 1.0,
        "accuracy": 1.0,
        "recommended": true,
        "auxiliary": false,
        "runsOn": ["macos", "windows", "linux"],
        "purposes": [{ "label": "Everyday dictation", "basis": "inferred", "evidence": null }],
        "description": "Test ASR row."
      }
    ]"#
    .to_string()
}

fn good_llm_body() -> String {
    r#"[
      {
        "id": "tiny-llm",
        "name": "Tiny LLM",
        "sizeLabel": "1B",
        "capabilities": ["polish"],
        "url": "https://example.invalid/tiny-llm.gguf",
        "sizeMb": 1,
        "description": "Test row.",
        "specialty": "cleanup",
        "sha256": "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634",
        "backupUrl": null,
        "attribution": "Test",
        "licenseName": "Apache-2.0",
        "licenseUrl": "https://www.apache.org/licenses/LICENSE-2.0",
        "requiresLicenseAccept": false,
        "recommended": true,
        "runsOnCpu": true,
        "verdict": "recommended",
        "verdictNote": "Test verdict.",
        "sortOrder": 1,
        "shards": [],
        "runsOn": ["macos", "windows", "linux"]
      }
    ]"#
    .to_string()
}

fn good_command() -> String {
    r#"[
      {
        "id": "tiny-decider",
        "name": "Tiny Decider",
        "engine": "onnx",
        "capabilities": ["tool_calling"],
        "artifacts": [],
        "baseUrl": "https://example.invalid/tiny-decider/",
        "sizeMb": 1,
        "description": "Test decider.",
        "runsOn": ["macos", "windows"],
        "runsOnCpu": true,
        "sizeLabel": "1M",
        "specialty": "Command Mode decider",
        "sha256": null,
        "backupUrl": null,
        "attribution": "Test",
        "licenseName": "Apache-2.0",
        "licenseUrl": "https://www.apache.org/licenses/LICENSE-2.0",
        "requiresLicenseAccept": false,
        "recommended": false,
        "verdict": "not_tested",
        "verdictNote": "Test decider.",
        "sortOrder": 1,
        "shards": []
      }
    ]"#
    .to_string()
}

fn good_files() -> HashMap<String, String> {
    HashMap::from([
        ("speech.json".to_string(), good_speech()),
        ("llm.json".to_string(), good_llm_body()),
        ("command.json".to_string(), good_command()),
    ])
}

/// Same bodies as good_files; the manifest is what lies about the hashes.
fn bad_hash_files() -> HashMap<String, String> {
    good_files()
}

/// llm.json with a duplicate id: fails validation (invariant 1).
fn invalid_schema_files() -> HashMap<String, String> {
    let mut files = good_files();
    let row = r#"{"id": "dup", "name": "A", "sizeLabel": "1B", "capabilities": ["polish"], "url": "https://example.invalid/a.gguf", "sizeMb": 1, "description": "x", "specialty": "cleanup", "sha256": "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634", "backupUrl": null, "attribution": "T", "licenseName": "Apache-2.0", "licenseUrl": "https://www.apache.org/licenses/LICENSE-2.0", "requiresLicenseAccept": false, "recommended": true, "runsOnCpu": true, "verdict": "recommended", "verdictNote": "x", "sortOrder": 1, "shards": [], "runsOn": ["macos"]}"#;
    let broken = format!("[{row}, {row}]");
    files.insert("llm.json".to_string(), broken);
    files
}

fn manifest(version: u64, files: &HashMap<String, String>) -> String {
    let speech = files.get("speech.json").unwrap().clone();
    let llm = files.get("llm.json").unwrap().clone();
    let command = files.get("command.json").unwrap().clone();
    format!(
        r#"{{ "version": {version}, "files": {{ "speech": "{}", "llm": "{}", "command": "{}" }}, "url": "{}" }}"#,
        sha256_hex(&speech),
        sha256_hex(&llm),
        sha256_hex(&command),
        BASE_URL
    )
}

fn v1_manifest() -> String {
    manifest(1, &good_files())
}

fn v2_manifest() -> String {
    manifest(2, &good_files())
}

fn installed_version(data_dir: &Path) -> u64 {
    let p = working_copy_dir(data_dir).join("version");
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

#[test]
fn a_newer_manifest_updates_the_working_copy() {
    let dir = tempfile::tempdir().unwrap();
    seed_from_bundled(&working_copy_dir(dir.path()));
    // R11: installed version file is absent at seed time => 0. Install v1
    // first so the v2 manifest is genuinely newer, then verify the swap.
    let files = good_files();
    let res = try_update(
        dir.path(),
        &StubFetch {
            manifest: v1_manifest(),
            files: files.clone(),
        },
    );
    assert!(matches!(res, UpdateResult::Updated), "v1 install");
    assert_eq!(installed_version(dir.path()), 1);
    // Reset the throttle so the next call is not Skipped.
    let old = SystemTime::now() - Duration::from_secs(7200);
    write_last_check(dir.path(), old);
    // A same-version manifest is Current, not Updated.
    let res = try_update(
        dir.path(),
        &StubFetch {
            manifest: v1_manifest(),
            files: files.clone(),
        },
    );
    assert!(matches!(res, UpdateResult::Current), "same version");
    // Now the newer v2 manifest must update the working copy.
    let old = SystemTime::now() - Duration::from_secs(7200);
    write_last_check(dir.path(), old);
    let res = try_update(
        dir.path(),
        &StubFetch {
            manifest: v2_manifest(),
            files: files.clone(),
        },
    );
    assert!(matches!(res, UpdateResult::Updated));
    assert_eq!(installed_version(dir.path()), 2);
    let wc = working_copy_dir(dir.path());
    assert_eq!(
        std::fs::read_to_string(wc.join("speech.json")).unwrap(),
        good_speech()
    );
    // last_check must be written so a follow-up call is throttled.
    assert!(last_check_path(dir.path()).exists());
}

#[test]
fn a_wrong_sha256_keeps_the_old_catalog() {
    let dir = tempfile::tempdir().unwrap();
    seed_from_bundled(&working_copy_dir(dir.path()));
    let wc = working_copy_dir(dir.path());
    let before_speech = std::fs::read_to_string(wc.join("speech.json")).unwrap();
    // Manifest claims hashes that do not match the fetched bodies.
    let lying_manifest = format!(
        r#"{{ "version": 2, "files": {{ "speech": "{}", "llm": "{}", "command": "{}" }}, "url": "{}" }}"#,
        "0".repeat(64),
        "0".repeat(64),
        "0".repeat(64),
        BASE_URL
    );
    let res = try_update(
        dir.path(),
        &StubFetch {
            manifest: lying_manifest,
            files: bad_hash_files(),
        },
    );
    assert!(matches!(res, UpdateResult::Failed));
    assert_eq!(
        std::fs::read_to_string(wc.join("speech.json")).unwrap(),
        before_speech,
        "failed update must leave the working copy untouched"
    );
    assert_eq!(installed_version(dir.path()), 0, "version must not bump");
}

#[test]
fn a_schema_error_keeps_the_old_catalog() {
    let dir = tempfile::tempdir().unwrap();
    seed_from_bundled(&working_copy_dir(dir.path()));
    let wc = working_copy_dir(dir.path());
    let before_llm = std::fs::read_to_string(wc.join("llm.json")).unwrap();
    // Hashes match the fetched bodies, but llm.json fails validation
    // (duplicate id).
    let files = invalid_schema_files();
    let res = try_update(
        dir.path(),
        &StubFetch {
            manifest: manifest(2, &files),
            files,
        },
    );
    assert!(matches!(res, UpdateResult::Failed));
    assert_eq!(
        std::fs::read_to_string(wc.join("llm.json")).unwrap(),
        before_llm,
        "failed update must leave the working copy untouched"
    );
    assert_eq!(installed_version(dir.path()), 0, "version must not bump");
}

#[test]
fn a_recent_check_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    write_last_check(dir.path(), SystemTime::now());
    let res = try_update(
        dir.path(),
        &StubFetch {
            manifest: v2_manifest(),
            files: good_files(),
        },
    );
    assert!(matches!(res, UpdateResult::Skipped));
}
