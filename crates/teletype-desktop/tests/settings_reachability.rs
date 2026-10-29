//! Settings reachability guard.
//!
//! claim: ipc-contract
//!
//! Every field on `commands::Settings` is persisted to `settings.json`,
//! deserialised at startup, and round-tripped through the `get_settings` /
//! `save_settings` / `update_settings_patch` IPC. That plumbing makes a field
//! *look* alive: the struct parser sees it, serde keeps it in the file, the UI
//! can write it. None of that is a consumer.
//!
//! This test parses the `Settings` struct out of `crates/teletype-desktop/src/
//! commands.rs`, then scans the production Rust sources (`crates/*/src`, test
//! modules stripped) and the UI sources (`ui/src`) for a *read* of each
//! field, excluding the declaration sites — the `Settings` struct body, the
//! `SettingsPatch` struct body, `impl Default for Settings`,
//! `impl Settings { fn migrate }`, the `SettingsPatch::apply_to` patch macro,
//! the `get_settings` body, and `update_settings` plumbing in `lib.rs` and
//! `commands/settings.rs`.
//!
//! A field is reachable when production code reads it in Rust (`s.field`,
//! `self.field`, `settings.field`, or a struct-literal `field:` key) or the UI
//! reads it (`.field` on the settings object, or a quoted `"field"` key). A
//! field with no such read is dead: it is persisted and savable but nothing
//! branches on it.
//!
//! Dead fields go in `KNOWN_UNREACHABLE` with a one-line reason, so the list
//! shrinks as engine paths land. A fresh (unlisted) dead field fails the
//! build — that is the point.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // crates/teletype-desktop -> crates -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

fn to_camel(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse `pub struct Settings { .. }` out of `commands.rs`, returning
/// `[(rust_field, wire_key)]`. The struct is `#[serde(rename_all =
/// "camelCase")]`, so the wire key is the camelCase form of the field name.
///
/// The parser tracks `#[serde(...)]` attributes above each field to honour
/// per-field `rename` and `skip` attributes (a `skip`ped field is not
/// persisted, so it is not part of the reachability contract).
fn parse_settings_fields(commands_src: &str) -> Vec<(String, String)> {
    let needle = "pub struct Settings {";
    let start = commands_src
        .find(needle)
        .expect("pub struct Settings not found in commands.rs");
    let body_start = start + needle.len();
    let end = body_start
        + commands_src[body_start..]
            .find("\n}")
            .expect("end of Settings struct");
    let body = &commands_src[body_start..end];

    // Confirm the struct is camelCase on the wire, so `to_camel` is correct.
    let attrs_start = commands_src[..start].rfind("#[serde").unwrap_or(start);
    let attrs = &commands_src[attrs_start..start];
    assert!(
        attrs.contains("rename_all = \"camelCase\""),
        "Settings struct lost its camelCase serde attribute; the wire-key \
         derivation in this test is now wrong"
    );

    let mut fields = Vec::new();
    // A `#[serde(...)]` line applies to the next field: `skip` drops it from
    // the persisted contract, `rename` overrides the camelCase wire key.
    let mut pending: Option<String> = None;
    for line in body.lines() {
        let l = line.trim();
        if let Some(attr) = l
            .strip_prefix("#[serde(")
            .and_then(|a| a.strip_suffix(")]"))
        {
            pending = Some(attr.to_string());
            continue;
        }
        if l.is_empty() || l.starts_with("//") || l.starts_with("///") {
            continue;
        }
        if l.starts_with("#[") {
            continue;
        }
        if let Some(rest) = l.strip_prefix("pub ") {
            let attr = pending.take();
            let is_skipped = attr.as_deref().is_some_and(|a| {
                a.split(',').any(|part| {
                    let p = part.trim();
                    p == "skip" || p.starts_with("skip_")
                })
            });
            if is_skipped {
                continue;
            }
            let (fname, _ty) = match rest.split_once(':') {
                Some(v) => v,
                None => continue,
            };
            let fname = fname.trim().to_string();
            if fname
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_')
            {
                let wire = attr
                    .as_deref()
                    .and_then(|a| {
                        a.split(',').map(|p| p.trim()).find_map(|p| {
                            p.strip_prefix("rename = \"")
                                .and_then(|r| r.strip_suffix('"'))
                        })
                    })
                    .map(str::to_string)
                    .unwrap_or_else(|| to_camel(&fname));
                fields.push((fname, wire));
            }
        }
    }
    fields
}

/// Strip `#[cfg(test)]` blocks from Rust source. A `#[cfg(test)]` line is
/// followed by the item it guards (a `mod tests`, a `fn`, a field, an impl,
/// etc.); the block runs until the next non-blank line whose indent is at or
/// below the guarded item's indent.
fn strip_cfg_test(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let n = lines.len();
    let mut out: Vec<String> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let line = lines[i];
        if line.trim() == "#[cfg(test)]" {
            // Find the indent of the guarded item.
            let mut j = i + 1;
            while j < n && lines[j].trim().is_empty() {
                j += 1;
            }
            if j >= n {
                break;
            }
            let item_indent = lines[j].len() - lines[j].trim_start().len();
            // Skip the guarded item: from `j` until a non-blank line with
            // indent <= item_indent.
            i = j + 1;
            while i < n {
                let l = lines[i];
                if !l.trim().is_empty() {
                    let ind = l.len() - l.trim_start().len();
                    if ind <= item_indent {
                        break;
                    }
                }
                i += 1;
            }
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }
    out.join("\n")
}

/// For `commands.rs` only: additionally strip the declaration/plumbing blocks
/// that are not consumers — the `Settings` struct body, the `SettingsPatch`
/// struct body, `impl Default for Settings`, `impl Settings` (migrate),
/// `impl SettingsPatch` (apply_to + the `apply!` macro), and the
/// `fn migrate_unavailable_speech_model` / `migrate_removed_models` bodies
/// (they write `settings.pending_notice` / `settings.selected_*` to inform
/// the user of a migration, which is a write, not a read of the user's
/// choice).
fn strip_declaration_blocks(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let n = lines.len();
    let mut out: Vec<String> = Vec::with_capacity(n);
    let mut i = 0;
    let mut skip_depth: i32 = 0;
    let mut skipping = false;

    while i < n {
        let line = lines[i];
        let t = line.trim();

        if !skipping {
            let starts_block = t == "pub struct Settings {"
                || t == "pub struct SettingsPatch {"
                || t.starts_with("impl Default for Settings")
                || t.starts_with("impl Settings {")
                || t.starts_with("impl SettingsPatch")
                || t.starts_with("pub fn migrate_unavailable_speech_model(")
                || t.starts_with("pub fn migrate_removed_models(")
                || t.starts_with("pub fn speech_migration_notice(");
            if starts_block {
                skipping = true;
                skip_depth = 0;
                // Count braces on the starting line.
                for c in line.chars() {
                    if c == '{' {
                        skip_depth += 1;
                    } else if c == '}' {
                        skip_depth -= 1;
                    }
                }
                i += 1;
                continue;
            }
        }

        if skipping {
            for c in line.chars() {
                if c == '{' {
                    skip_depth += 1;
                } else if c == '}' {
                    skip_depth -= 1;
                }
            }
            if skip_depth <= 0 {
                skipping = false;
            }
            i += 1;
            continue;
        }

        out.push(line.to_string());
        i += 1;
    }
    out.join("\n")
}

/// Walk `dir` for `.rs` files, skipping any `tests/` directory.
fn collect_rust(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().map(|s| s == "tests").unwrap_or(false) {
                continue;
            }
            collect_rust(&p, out);
        } else if p.extension().map(|s| s == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
}

/// Walk `dir` for `.ts` / `.tsx` files.
fn collect_ui(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_ui(&p, out);
        } else if matches!(p.extension().and_then(|s| s.to_str()), Some("ts" | "tsx")) {
            out.push(p);
        }
    }
}

/// Paths (as repo-relative strings) whose reads are declaration/plumbing,
/// not consumers. These files *define* or *round-trip* the settings document
/// but do not branch on a field's value.
fn is_declaration_file(rel: &str) -> bool {
    // commands.rs is handled by strip_declaration_blocks; it is still a
    // declaration file for the purpose of "a read here is not a consumer"
    // when the read is inside a block we did not strip (e.g. a doc comment
    // that mentions `settings.field`).
    matches!(
        rel,
        "crates/teletype-desktop/src/commands.rs"
            | "crates/teletype-desktop/src/commands/settings.rs"
            | "crates/teletype-desktop/src/lib.rs"
            | "crates/teletype-desktop/tests/migration.rs"
    )
}

#[test]
fn every_settings_field_has_a_production_reader() {
    let root = repo_root();

    // --- parse the Settings struct ---
    let commands_path = root.join("crates/teletype-desktop/src/commands.rs");
    let commands_src = std::fs::read_to_string(&commands_path).expect("read commands.rs");
    let fields = parse_settings_fields(&commands_src);
    assert!(
        fields.len() >= 60,
        "parsed {} Settings fields; expected at least 60 — the parser \
         regressed or the struct was split",
        fields.len()
    );

    // --- collect production Rust sources (test modules stripped) ---
    let mut rust_files: Vec<PathBuf> = Vec::new();
    for rel in [
        "crates/teletype-desktop/src",
        "crates/teletype-core/src",
        "crates/teletype-inference/src",
        "crates/teletype-speech/src",
        "crates/teletype-eval/src",
        "crates/teletype-decider/src",
        "crates/teletype-fluid/src",
        "crates/tinydecide/src",
    ] {
        collect_rust(&root.join(rel), &mut rust_files);
    }
    assert!(
        !rust_files.is_empty(),
        "no Rust sources found — repo layout changed"
    );

    // Build a map of repo-relative path -> test-stripped source. For
    // commands.rs, also strip the declaration blocks.
    let mut per_file: Vec<(String, String)> = Vec::new();
    for f in &rust_files {
        let rel = f
            .strip_prefix(&root)
            .expect("rust file under repo root")
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(f).expect("read rust file");
        let src = strip_cfg_test(&src);
        let src = if rel == "crates/teletype-desktop/src/commands.rs" {
            strip_declaration_blocks(&src)
        } else {
            src
        };
        per_file.push((rel, src));
    }

    // --- collect UI sources ---
    let mut ui_files: Vec<PathBuf> = Vec::new();
    collect_ui(&root.join("ui/src"), &mut ui_files);
    assert!(!ui_files.is_empty(), "no UI sources found");
    let mut ui_text = String::new();
    for f in &ui_files {
        ui_text.push_str(&std::fs::read_to_string(f).expect("read ui file"));
        ui_text.push('\n');
    }

    // --- scan for a reader of each field ---
    let mut unreachable: Vec<String> = Vec::new();
    let mut reachable: Vec<String> = Vec::new();
    for (snake, camel) in &fields {
        let mut found = false;

        // Rust production read: `.<snake>` or a struct-literal `snake:` key,
        // outside declaration files. A line starting `pub ` or `//` is not a
        // read (declaration or comment).
        if !found {
            for (rel, text) in &per_file {
                if is_declaration_file(rel) {
                    continue;
                }
                for line in text.lines() {
                    let t = line.trim_start();
                    if t.starts_with("pub ") || t.starts_with("//") {
                        continue;
                    }
                    if t.contains(&format!(".{snake}")) || t.contains(&format!("{snake}:")) {
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }
        }

        // UI read: `.camel` on the settings object, or a quoted `"camel"` key
        // (the UI types the settings interface, so the wire key is camelCase).
        if !found
            && (ui_text.contains(&format!(".{camel}")) || ui_text.contains(&format!("\"{camel}\"")))
        {
            found = true;
        }

        if found {
            reachable.push(camel.clone());
        } else {
            unreachable.push(camel.clone());
        }
    }

    // --- ratchet: known-unreachable fields are allowed, fresh ones fail ---
    //
    // A field is KNOWN_UNREACHABLE when it is persisted and the UI can write
    // it, but no production code reads its value to change behaviour. Each
    // entry carries a one-line reason so the next engineer knows whether to
    // wire the field up or delete it. The list must shrink: a field that
    // gains a production reader is removed from this list in the same
    // change.
    const KNOWN_UNREACHABLE: &[&str] = &[
        // P5.7 take-quality / instant-start unit: the capture path must
        // consult this before resampling when a BT mic is present. Not yet
        // implemented; the field is persisted so the UI can save the choice.
        "preferBuiltinMicWithBt",
        // P5.5 DeepFilter pipeline split: toggles the raw-vs-DFN3 split in
        // capture. Not yet implemented; the field is persisted so the UI can
        // save the choice.
        "audioConditioning",
    ];

    let fresh: Vec<&String> = unreachable
        .iter()
        .filter(|f| !KNOWN_UNREACHABLE.contains(&f.as_str()))
        .collect();
    assert!(
        fresh.is_empty(),
        "unreachable settings field(s) with no production reader: {fresh:#?}.\n\
         Each is persisted to settings.json and savable from the UI, but no \
         production Rust or UI code reads its value to change behaviour — \
         it is a dead control (the BUG-002/BUG-003/BUG-010 class). Either \
         wire the field into the engine path (a Rust consumer) or the UI, \
         or delete the field and its settings.json entry. See \
         .agents/brain/learnings/2026-09-28-advertised-features-without-\
         engine-path.md (L011).",
    );

    // Stale entries in KNOWN_UNREACHABLE (a field that now has a reader) are
    // a code-smell the test should flag, so the list does not rot.
    let stale: Vec<&str> = KNOWN_UNREACHABLE
        .iter()
        .copied()
        .filter(|k| reachable.iter().any(|r| r == k))
        .collect();
    assert!(
        stale.is_empty(),
        "stale KNOWN_UNREACHABLE entries (field now has a production reader): \
         {stale:#?}. Remove them from the allowlist.",
    );

    eprintln!(
        "settings_reachability: {} fields, {} reachable, {} unreachable \
         ({} known), {} fresh",
        fields.len(),
        reachable.len(),
        unreachable.len(),
        KNOWN_UNREACHABLE.len(),
        fresh.len(),
    );
}
