//! IPC contract smoke tests: the Rust <-> TypeScript boundary.
//!
//! `npx tsc --noEmit` passes on this repo while four user-facing features are
//! silently broken, because every TypeScript interface is hand-declared and
//! `tsc` has no way to know what serde actually emits. The rules are simple
//! and total:
//!
//!   * a struct that crosses the boundary serialises with
//!     `#[serde(rename_all = "camelCase")]`, so its JSON keys are the Rust
//!     field names in camelCase;
//!   * `invoke("<cmd>")` must name a `#[tauri::command]` that exists.
//!
//! Both are checkable mechanically, so they are checked mechanically here.
//! The four bugs this file was written for:
//!
//!   BUG-06  `create_transform` sends `sort_order`/`auto_apply`/`built_in`/
//!           `created_at`/`updated_at`, so serde reports "missing field
//!           builtIn" and the "+ Create New" button never works.
//!   BUG-07  `add_preference` sends `created_at`/`updated_at`, same failure.
//!   BUG-08  the `Profile` interface reads `learn_from_edits`; the wire key is
//!           `learnFromEdits`, so all three learning toggles render OFF and
//!           cross-clobber each other.
//!   BUG-09  the `Transform` interface reads `t.built_in`, which is always
//!           `undefined`, so `!t.built_in` is always true and a Delete button
//!           is drawn on every shipped transform.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// locating the two trees
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    // crates/teletype-core -> crates -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn ui_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if matches!(p.extension().and_then(|s| s.to_str()), Some("ts" | "tsx")) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&repo_root().join("ui/src"), &mut out);
    out.sort();
    assert!(!out.is_empty(), "no UI sources found");
    out
}

// ---------------------------------------------------------------------------
// minimal parsers
// ---------------------------------------------------------------------------

/// Extract the body of `#[serde(...)]`-annotated `pub struct <name> { .. }`
/// from Rust source, returning `(camel_case: bool, [(rust_field, json_key)])`.
fn rust_struct_fields(src: &str, name: &str) -> (bool, Vec<(String, String)>) {
    let needle = format!("pub struct {name} {{");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("struct {name} not found"));
    let body_start = start + needle.len();
    let end = src[body_start..].find("\n}").expect("struct end") + body_start;

    // The attribute block sits immediately above the struct.
    let attrs_start = src[..start].rfind("#[serde").unwrap_or(start);
    let attrs = &src[attrs_start..start];
    let camel = attrs.contains("rename_all = \"camelCase\"")
        || attrs.contains("rename_all = \"camelCase\", try_from")
        || attrs.contains("rename_all = \"kebab-case\"");

    let mut fields = Vec::new();
    for line in src[body_start..end].lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("//") || l.starts_with("///") {
            continue;
        }
        if l.starts_with("#[") {
            continue;
        }
        if let Some(rest) = l.strip_prefix("pub ") {
            let (fname, _ty) = match rest.split_once(':') {
                Some(v) => v,
                None => continue,
            };
            let fname = fname.trim().to_string();
            if !fname
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_')
            {
                continue;
            }
            let key = if camel {
                to_camel(&fname)
            } else {
                fname.clone()
            };
            fields.push((fname, key));
        }
    }
    (camel, fields)
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

/// Every `invoke("name")` / `invoke<...>("name")` command name in the UI.
fn ui_invoke_names() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for f in ui_files() {
        let src = std::fs::read_to_string(&f).expect("read ui file");
        let src_bytes = src.as_bytes();
        let mut i = 0usize;
        while let Some(pos) = src[i..].find("invoke") {
            let at = i + pos; // byte offset of 'i' in "invoke"
                              // `invoke` must be a standalone identifier. Check the byte
                              // immediately before and after (ASCII-safe: identifiers are ASCII).
            let before_ok = at == 0
                || !src_bytes[at - 1].is_ascii_alphanumeric()
                    && src_bytes[at - 1] != b'_'
                    && src_bytes[at - 1] != b'.';
            let after = at + 6;
            let after_ok = after >= src_bytes.len()
                || !src_bytes[after].is_ascii_alphanumeric() && src_bytes[after] != b'_';
            // Skip the import line: `import { invoke } from "@tauri-apps/..."`
            // is not a call.
            let line_start = src[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let on_import = src[line_start..at].trim_start().starts_with("import");
            if before_ok && after_ok && !on_import {
                // Walk the real grammar: `invoke` ws [ `<` type `>` ] ws `(`
                // ws `"` name `"`. Anything else is not a call, and must not
                // be scanned forward for a stray string literal.
                let rest = &src[after..];
                let mut j = 0usize;
                let skip_ws = |s: &str, mut j: usize| {
                    while j < s.len() && s.as_bytes()[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    j
                };
                j = skip_ws(rest, j);
                if j < rest.len() && rest.as_bytes()[j] == b'<' {
                    // Walk the generic type parameter. Object-literal types
                    // (`invoke<{ seconds: number }>("cmd")`) contain `{`/`}`
                    // and `;` inside the `<...>`, so we must track brace depth
                    // and only bail on a top-level `;`.
                    let rb = rest.as_bytes();
                    let mut angle = 0i32;
                    let mut brace = 0i32;
                    while j < rb.len() {
                        match rb[j] {
                            b'<' => angle += 1,
                            b'>' => {
                                if brace == 0 {
                                    angle -= 1;
                                    if angle == 0 {
                                        j += 1;
                                        break;
                                    }
                                }
                            }
                            b'{' => brace += 1,
                            b'}' => brace = brace.saturating_sub(1),
                            b';' if brace == 0 => break,
                            _ => {}
                        }
                        j += 1;
                    }
                    j = skip_ws(rest, j);
                }
                if j < rest.len() && rest.as_bytes()[j] == b'(' {
                    j += 1;
                    j = skip_ws(rest, j);
                    if j < rest.len() && rest.as_bytes()[j] == b'"' {
                        let name_start = j + 1;
                        if let Some(end) = rest[name_start..].find('"') {
                            let name = &rest[name_start..name_start + end];
                            let ok = !name.is_empty()
                                && name.as_bytes()[0].is_ascii_lowercase()
                                && name.bytes().all(|c| {
                                    c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'
                                });
                            if ok {
                                out.insert(name.to_string());
                            }
                        }
                    }
                }
            }
            i = at + 6;
        }
    }
    out
}

/// Every function registered as a Tauri command in the backend.
fn backend_command_names() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let dir = repo_root().join("crates/teletype-desktop/src");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs(&dir, &mut files);
    assert!(!files.is_empty(), "no backend sources found");

    for p in files {
        let src = std::fs::read_to_string(&p).expect("read backend file");
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.trim().starts_with("#[tauri::command]") {
                continue;
            }
            // The signature is on this line or one of the next few, before the
            // body opens.
            for probe in lines.iter().skip(i).take(6).map(|x| x.trim()) {
                if probe.starts_with("#[") {
                    continue;
                }
                for kw in ["pub fn ", "pub async fn "] {
                    if let Some(rest) = probe.strip_prefix(kw) {
                        if let Some(name) = rest.split('(').next() {
                            let n = name.trim();
                            if !n.is_empty() {
                                out.insert(n.to_string());
                            }
                        }
                        break;
                    }
                }
                if probe.contains('{') {
                    break;
                }
            }
        }
    }
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------------------
// 1. every invoke() names a real command
// ---------------------------------------------------------------------------

#[test]
fn contract_01_every_ui_invoke_names_a_real_tauri_command() {
    let called = ui_invoke_names();
    let defined = backend_command_names();
    assert!(
        called.len() >= 40,
        "the invoke scanner only found {} unique command names; the parser is \
         broken, not the app",
        called.len()
    );
    assert!(
        defined.len() >= 70,
        "only {} backend commands parsed; the parser is broken, not the app",
        defined.len()
    );
    let missing: Vec<&String> = called.difference(&defined).collect();
    assert!(
        missing.is_empty(),
        "the UI calls commands that do not exist. Each one is a dead button:\n  {missing:#?}\n\
         (a command is only 'defined' if it is annotated #[tauri::command] AND registered \
         in lib.rs; check both before acting on this)"
    );
}

#[test]
fn contract_02_every_tauri_command_is_registered_in_lib_rs() {
    // A command that is annotated but not in the `generate_handler!` list is
    // callable from Rust and unreachable from the UI. That is how a feature
    // ships "wired in the backend" with no way to reach it.
    let defined = backend_command_names();
    let lib = read("crates/teletype-desktop/src/lib.rs");
    let mut registered = BTreeSet::new();
    for line in lib.lines() {
        let l = line.trim().trim_end_matches(',');
        // Entries look like `commands::get_settings,`
        if let Some(rest) = l.strip_prefix("commands::") {
            if !rest.contains('(') && !rest.contains(' ') && !rest.is_empty() {
                registered.insert(rest.to_string());
            }
        }
    }
    assert!(
        registered.len() >= 50,
        "only {} commands parsed out of generate_handler!, the parser is broken",
        registered.len()
    );
    let unregistered: Vec<&String> = defined.difference(&registered).collect();
    assert!(
        unregistered.is_empty(),
        "these commands are annotated #[tauri::command] but are NOT in the \
         generate_handler! list, so the UI can never call them:\n  {unregistered:#?}"
    );
}

// ---------------------------------------------------------------------------
// 2. TS interfaces must speak the wire's casing
// ---------------------------------------------------------------------------

/// (rust file, struct, ts file, ts interface)
const WIRE_TYPES: &[(&str, &str, &str, &str)] = &[
    (
        "crates/teletype-core/src/transforms/mod.rs",
        "TransformDefinition",
        "ui/src/screens/TransformsScreen.tsx",
        "Transform",
    ),
    (
        "crates/teletype-core/src/personalization/mod.rs",
        "Preference",
        "ui/src/screens/PersonalizationScreen.tsx",
        "Preference",
    ),
    (
        "crates/teletype-core/src/personalization/mod.rs",
        "UserProfile",
        "ui/src/screens/PersonalizationScreen.tsx",
        "Profile",
    ),
    (
        "crates/teletype-core/src/autotext/mod.rs",
        "AutoTextEntry",
        "ui/src/screens/AutoTextScreen.tsx",
        "Entry",
    ),
    (
        "crates/teletype-core/src/dictionary.rs",
        "DictionaryWord",
        "ui/src/screens/DictionaryScreen.tsx",
        "Word",
    ),
    (
        "crates/teletype-core/src/style.rs",
        "StyleProfile",
        "ui/src/screens/StylesScreen.tsx",
        "StyleProfile",
    ),
    (
        "crates/teletype-core/src/history.rs",
        "DictationEntry",
        "ui/src/screens/DictationScreen.tsx",
        "Entry",
    ),
    (
        "crates/teletype-core/src/scratchpad.rs",
        "ScratchEntry",
        "ui/src/screens/ScratchpadScreen.tsx",
        "ScratchEntry",
    ),
];

/// Casing defects that are present in the tree today. Each one is a real
/// broken feature; each is listed here so this test is a *ratchet*: CI stays
/// green, and any mismatch not on this list fails immediately. When you fix
/// one, delete its line and the test will hold you to it.
///
/// (`file`, `interface`, `field`, what the user sees)
const KNOWN_CASING_DEFECTS: &[(&str, &str, &str, &str)] = &[];

#[test]
fn contract_03_ts_interfaces_use_the_keys_serde_actually_emits() {
    let mut new_problems: Vec<String> = Vec::new();
    let mut checked = 0;

    for (rs_file, rs_struct, ts_file, ts_iface) in WIRE_TYPES {
        let rs = read(rs_file);
        let (camel, fields) = rust_struct_fields(&rs, rs_struct);
        assert!(
            !fields.is_empty(),
            "parsed no fields out of {rs_struct} in {rs_file}"
        );
        let wire: BTreeSet<String> = fields.iter().map(|(_, k)| k.clone()).collect();

        let ts = read(ts_file);
        let Ok(declared) = ts_interface_fields_opt(&ts, ts_iface) else {
            // The screen may consume a shared type instead of declaring one.
            continue;
        };
        checked += 1;

        // A TS field the wire never sends reads `undefined` forever, so every
        // branch keyed on it silently takes the falsy path.
        for f in declared.difference(&wire) {
            let known = KNOWN_CASING_DEFECTS
                .iter()
                .find(|(kf, ki, kd, _)| kf == ts_file && ki == ts_iface && kd == &f.as_str());
            let msg = match known {
                Some((_, _, _, impact)) => {
                    format!("KNOWN: {ts_file} interface {ts_iface}.{f} -> {impact}")
                }
                None => format!(
                    "NEW MISMATCH: {ts_file} interface {ts_iface} reads `{f}`, which the \
                     wire never sends. {rs_struct} emits {wire:?}. This is a dead control \
                     or a silently-falsy branch."
                ),
            };
            if known.is_none() {
                new_problems.push(msg);
            }
        }
        // A snake_case field on a camelCase struct fails on the way out too:
        // serde reports "missing field" and the whole write is rejected.
        if camel {
            for f in declared.iter().filter(|d| d.contains('_')) {
                if !KNOWN_CASING_DEFECTS
                    .iter()
                    .any(|(kf, ki, kd, _)| kf == ts_file && ki == ts_iface && kd == &f.as_str())
                {
                    new_problems.push(format!(
                        "NEW MISMATCH: {ts_file} interface {ts_iface}.{f} is snake_case but \
                         {rs_struct} is camelCase, so the key is dropped on read and the \
                         write is rejected on save."
                    ));
                }
            }
        }
    }

    assert!(
        checked >= 5,
        "only {checked} interfaces compared; parser regressed"
    );
    assert!(
        new_problems.is_empty(),
        "{} NEW IPC casing mismatch(es). Each is a feature the user cannot use, and \
         tsc cannot see it.\n\n  {}\n\nIf one of these is a known defect, add it to \
         KNOWN_CASING_DEFECTS with a one-line description of what the user sees.",
        new_problems.len(),
        new_problems.join("\n\n  ")
    );
}

fn ts_interface_fields_opt(src: &str, name: &str) -> Result<BTreeSet<String>, ()> {
    let needle = format!("interface {name} {{");
    let Some(start) = src.find(&needle) else {
        return Err(());
    };
    let body_start = start + needle.len();
    let Some(rel_end) = src[body_start..].find("\n}") else {
        return Err(());
    };
    let end = body_start + rel_end;
    let mut out = BTreeSet::new();
    for line in src[body_start..end].lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("//") {
            continue;
        }
        if let Some((field, _)) = l.split_once(':') {
            let f = field.trim().trim_end_matches('?').to_string();
            if !f.is_empty() && f.chars().all(|c| c.is_alphanumeric() || c == '_') {
                out.insert(f);
            }
        }
    }
    if out.is_empty() {
        return Err(());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 3. Settings: the one struct the TS interface must cover completely
// ---------------------------------------------------------------------------

#[test]
fn contract_04_the_settings_interface_covers_every_persisted_field() {
    // `Settings` is the widest struct on the boundary and the one the user
    // edits most. Every persisted field must be readable and writable from
    // the UI, or the user has no way to see or change it.
    let rs = read("crates/teletype-desktop/src/commands.rs");
    let (camel, fields) = rust_struct_fields(&rs, "Settings");
    assert!(camel, "Settings must serialise camelCase for the UI");
    assert!(
        fields.len() >= 20,
        "parsed only {} Settings fields; the parser regressed",
        fields.len()
    );
    let wire: BTreeSet<String> = fields.iter().map(|(_, k)| k.clone()).collect();

    let ts = read("ui/src/screens/SettingsScreen.tsx");
    let declared = ts_interface_fields_opt(&ts, "Settings").expect("Settings interface");
    let unknown: Vec<&String> = declared.difference(&wire).collect();
    assert!(
        unknown.is_empty(),
        "the Settings interface reads key(s) {unknown:?} the backend never sends"
    );

    // A field the UI cannot see is a feature the user cannot control. The
    // known intentional exceptions are listed explicitly so adding a new one
    // is a deliberate act rather than an accident.
    let invisible: Vec<&String> = wire
        .difference(&declared)
        .filter(|w| !SETTINGS_WITHOUT_UI.iter().any(|(k, _)| k == &w.as_str()))
        .collect();
    assert!(
        invisible.is_empty(),
        "persisted setting(s) {invisible:?} have no control anywhere in the UI. The \
         feature list promises them to the user; there is no way to read or change \
         them. Add a control, or add the key to SETTINGS_WITHOUT_UI with a reason."
    );
}

/// Persisted settings with no UI today. Each is a ratchet entry, not an
/// endorsement: the setting works, the user just cannot reach it.
const SETTINGS_WITHOUT_UI: &[(&str, &str)] = &[
    (
        "activeStyleProfile",
        "set from the Styles screen via a command, not a Settings field",
    ),
    ("enabledPacks", "toggled from the Dictionary screen"),
    (
        "openaiBaseUrl",
        "set from the Models screen connector panel",
    ),
    ("openaiModel", "set from the Models screen connector panel"),
    (
        "selectedLlmProvider",
        "set by selecting a model in the Models screen",
    ),
    (
        "scratchpadEnabled",
        "P3.x placeholder, never read by the pipeline",
    ),
    ("restoreEmoji", "P3.2 shipped with no toggle"),
    ("polishGateEnabled", "P5.1 shipped with no toggle"),
    ("polishGateThresholdWords", "P5.1 shipped with no toggle"),
    // P3.3 per-app language: four registered commands
    // (set/get_app_language_override(s)) with zero UI callers, so the feature
    // is unreachable even though the brain lists it as shipped.
    (
        "appLanguageOverrides",
        "P3.3 shipped with no UI; set/get commands have no callers",
    ),
];

// ---------------------------------------------------------------------------
// 4. the "no model loaded" signal must be observable
// ---------------------------------------------------------------------------

#[test]
fn contract_05_the_no_model_signal_reaches_a_user_visible_surface() {
    // A user cannot distinguish "polish is working" from "polish is dead"
    // unless something says so. Trace the signal end to end so a future
    // refactor cannot quietly delete the last surface that reports it.
    let pipeline = read("crates/teletype-core/src/pipeline.rs");
    assert!(
        pipeline.contains("transform_skipped_no_model"),
        "PipelineResult lost transform_skipped_no_model"
    );
    // The producer must set it on the no-provider path.
    let no_model_arm = pipeline.find("NoModelLoaded").expect("no-model arm");
    let flag_line = pipeline
        .find("let transform_skipped_no_model")
        .expect("flag computation");
    assert!(
        no_model_arm < flag_line,
        "BUG-05 has returned: the no-model arm runs before the flag is computed"
    );

    let dictation = read("crates/teletype-desktop/src/dictation.rs");
    assert!(
        dictation.contains("result.transform_skipped_no_model"),
        "the desktop controller no longer reads the no-model signal"
    );
    assert!(
        dictation.contains("skip_reason"),
        "the desktop controller no longer reads metrics.skip_reason either; \
         nothing reports a dead polish path to the user"
    );
}

// ---------------------------------------------------------------------------
// 5. Every persisted setting must have a production caller (BUG-019)
// ---------------------------------------------------------------------------
//
// The 2026-09-28 QA audit found settings that persisted to disk but were read
// exactly once at startup (BUG-010), never read at all (BUG-003's timer), or
// had no engine path behind the advertised feature (BUG-002). A setting with
// zero production callers is a dead control; a toggle that only applies at
// restart is a silent no-op. This test greps the production tree for a read
// of each `Settings` field and fails on any field whose only references are
// its declaration, its default, and its serde attribute.

/// Settings with no production caller today. Each is a ratchet entry, not an
/// endorsement: the setting persists, production just doesn't read it yet.
const SETTINGS_WITHOUT_PRODUCTION_CALLER: &[(&str, &str)] = &[];

#[test]
fn contract_06_every_setting_has_a_production_caller() {
    let rs = read("crates/teletype-desktop/src/commands.rs");
    let (_, fields) = rust_struct_fields(&rs, "Settings");
    assert!(
        !fields.is_empty(),
        "parsed zero Settings fields; the parser regressed"
    );

    // The production tree: the four crate src/ dirs, excluding tests/.
    let mut srcs: Vec<String> = Vec::new();
    fn collect(dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().map(|n| n == "tests").unwrap_or(false) {
                    continue;
                }
                collect(&p, out);
            } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    out.push(s);
                }
            }
        }
    }
    let root = repo_root();
    for crate_dir in [
        "crates/teletype-desktop/src",
        "crates/teletype-core/src",
        "crates/teletype-inference/src",
        "crates/teletype-speech/src",
    ] {
        collect(&root.join(crate_dir), &mut srcs);
    }
    let tree: String = srcs.join("\n");

    let mut dead: Vec<String> = Vec::new();
    for (snake, camel) in &fields {
        // A production caller reads the field: `settings.<snake>`,
        // `s.<snake>`, `self.settings().<snake>`, or the struct-literal
        // default `has_completed_onboarding: false,`. Bare field-name
        // mentions (prose, serde attributes) must not count.
        let reads = tree
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                (t.contains(&format!(".{snake}")) || t.contains(&format!("{snake}:")))
                    && !t.starts_with("pub ")
                    && !t.starts_with("//")
                    && !t.starts_with("///")
            })
            .count();
        if reads == 0
            && !SETTINGS_WITHOUT_PRODUCTION_CALLER
                .iter()
                .any(|(_, k)| k == camel)
        {
            dead.push(camel.clone());
        }
    }
    assert!(
        dead.is_empty(),
        "setting(s) {dead:?} are persisted and exposed to the UI but never read \n\
         by production code. A setting with no caller is a dead control (the \n\
         BUG-010/BUG-003 class). Wire it into the engine path, or list it in \n\
         SETTINGS_WITHOUT_PRODUCTION_CALLER with a reason."
    );
}

// ---------------------------------------------------------------------------
// 6. BUG-019 (part 1): every persisted setting has a production reader
// ---------------------------------------------------------------------------
//
// contract_06 above counts declaration/default/save lines as "reads", so a
// setting that is only stored and persisted looks alive. This test excludes
// the `Settings` declaration block, the `Default for Settings` impl, and the
// `save_settings` command body, and requires at least one remaining read. A
// field that survives only in the write path is a silent no-op (BUG-002/003).

/// (file, in declaration block, in Default impl, in save_settings body)
fn read_count_in_file(path: &str, snake: &str, camel: &str) -> usize {
    let rs = read(path);
    let mut in_decl = false;
    let mut in_default = false;
    let mut in_save = false;
    let mut depth = 0i32;
    let mut reads = 0usize;
    for line in rs.lines() {
        let t = line.trim();
        if !in_decl && !in_default && !in_save {
            if t == "pub struct Settings {" {
                in_decl = true;
            } else if t.starts_with("impl Default for Settings") {
                in_default = true;
            } else if t.starts_with("pub async fn save_settings(") {
                in_save = true;
                depth = 0;
            }
        }
        if in_decl {
            if t == "}" {
                in_decl = false;
            }
            continue;
        }
        if in_default {
            if t == "}" {
                in_default = false;
            }
            continue;
        }
        if in_save {
            let opens = t.chars().filter(|&c| c == '{').count() as i32;
            let closes = t.chars().filter(|&c| c == '}').count() as i32;
            depth += opens - closes;
            if depth <= 0 {
                in_save = false;
            }
            continue;
        }
        if t.starts_with("pub ") || t.starts_with("//") || t.starts_with("///") {
            continue;
        }
        // A read: field access `.<snake>` or a struct-literal `snake:` key
        // outside the three excluded regions.
        if t.contains(&format!(".{snake}")) || t.contains(&camel.to_string()) {
            reads += 1;
        }
    }
    reads
}

/// The engine path lives in dictation.rs and lib.rs as much as in
/// commands.rs; a setting read from any of them counts.
fn settings_read_apply_callers(snake: &str, camel: &str) -> usize {
    read_count_in_file("crates/teletype-desktop/src/commands.rs", snake, camel)
        + read_count_in_file("crates/teletype-desktop/src/dictation.rs", snake, camel)
        + read_count_in_file("crates/teletype-desktop/src/lib.rs", snake, camel)
}

/// Fields that are legitimately read-only at runtime (onboarding state, or a
/// value that is applied by the UI itself, not by Rust). Deliberate
/// exceptions; keep this list as short as possible.
const SETTINGS_UI_ONLY: &[&str] = &[
    "hasCompletedOnboarding",
    // The recording pill is a separate webview that fetches its own settings
    // via get_settings; it reads these keys from the wire, not from a Rust
    // field access in this crate's src tree.
    "recordingMode",
    "keepTextOnClipboard",
    "alwaysShowPill",
    "enableDeveloperTab",
    "pillStyle",
    // P3.x placeholder, never read by the pipeline (see SETTINGS_WITHOUT_UI).
    "scratchpadEnabled",
    // The AI Polish rule toggles are composed into the transform instruction
    // by the UI (TransformsScreen.tsx); Rust never reads the field directly.
    "polishRules",
];

#[test]
fn contract_07_every_persisted_setting_has_a_production_caller() {
    let rs = read("crates/teletype-desktop/src/commands.rs");
    let (_, fields) = rust_struct_fields(&rs, "Settings");
    assert!(
        !fields.is_empty(),
        "parsed zero Settings fields; the parser regressed"
    );

    let mut dead: Vec<String> = Vec::new();
    for (snake, camel) in &fields {
        if SETTINGS_UI_ONLY.contains(&camel.as_str()) {
            continue;
        }
        let reads = settings_read_apply_callers(snake, camel);
        if reads == 0 {
            dead.push(camel.clone());
        }
    }
    assert!(
        dead.is_empty(),
        "setting(s) {dead:?} are persisted and saved but have no read/apply \
         call site in production code (outside the Settings declaration, its \
         Default impl, and save_settings). A setting that is only written is a \
         silent no-op (the BUG-002/BUG-003 class). Wire it into the engine \
         path, or add the key to SETTINGS_UI_ONLY with a reason."
    );
}

// ---------------------------------------------------------------------------
// 7. BUG-019 (part 2): every Screen variant is reachable from the nav
// ---------------------------------------------------------------------------

fn ts_union_variants(src: &str, name: &str) -> Vec<String> {
    // `type <name> = "a" | "b" | ...;` — the union may span lines, so the
    // quoted strings are collected between the `=` and the terminating `;`.
    let needle = format!("type {name} =");
    let Some(start) = src.find(&needle) else {
        return Vec::new();
    };
    let rest = &src[start..];
    let Some(end) = rest.find(';') else {
        return Vec::new();
    };
    let body = &rest[..end];
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let Some(close) = bytes[start..].iter().position(|&b| b == b'"') else {
                break;
            };
            out.push(body[start..start + close].to_string());
            i = start + close + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn nav_entry_ids(src: &str) -> Vec<String> {
    // NAV array: `{ id: "home", ... }` entries. The declaration line reads
    // `const NAV_BASE: { id: Screen; ... }[] = [`, so the body starts at the
    // first `[` on the line AFTER the type annotation, not the `[` of the
    // type itself.
    let Some(start) = src.find("NAV_BASE") else {
        return Vec::new();
    };
    let rest = &src[start..];
    let Some(eq) = rest.find('=') else {
        return Vec::new();
    };
    let after_eq = &rest[eq + 1..];
    let Some(bracket) = after_eq.find('[') else {
        return Vec::new();
    };
    let after = &after_eq[bracket + 1..];
    let Some(end) = after.find(']') else {
        return Vec::new();
    };
    let body = &after[..end];
    let mut out = Vec::new();
    for line in body.lines() {
        if let Some(idx) = line.find("id:") {
            let after = &line[idx + 3..];
            if let Some(q1) = after.find('"') {
                if let Some(q2) = after[q1 + 1..].find('"') {
                    out.push(after[q1 + 1..q1 + 1 + q2].to_string());
                }
            }
        }
    }
    out
}

#[test]
fn contract_08_every_nav_screen_is_reachable() {
    let src = read("ui/src/App.tsx");
    let screens = ts_union_variants(&src, "Screen");
    assert!(
        screens.len() >= 10,
        "parsed only {} Screen variants from App.tsx; the parser regressed",
        screens.len()
    );
    let nav = nav_entry_ids(&src);
    assert!(
        !nav.is_empty(),
        "parsed no NAV entries from App.tsx; the parser regressed"
    );
    let unreachable: Vec<&String> = screens
        .iter()
        .filter(|s| s.as_str() != "developer") // dev-gated by design
        .filter(|s| !nav.iter().any(|n| n == *s))
        .collect();
    assert!(
        unreachable.is_empty(),
        "Screen variant(s) {unreachable:?} are rendered but have no NAV entry, \
         so the user cannot reach them (BUG-012: the Personalization screen was \
         shipped with no nav item). Add the entry to NAV_BASE in ui/src/App.tsx."
    );
}

// ---------------------------------------------------------------------------
// 8. BUG-019 (part 3): every advertised feature has an engine path
// ---------------------------------------------------------------------------
//
// Curated list, derived from the brain decision log (the 2026-09-25 EW
// settings port) and the roadmap. Each entry names the setting (camelCase,
// as persisted) and/or the tauri command that is the feature's engine path.
// A feature that has neither is advertised in the brain/README with nothing
// behind it (BUG-002: spoken emoji/punctuation; BUG-003: unload-model-after).
// When a feature lands, its entry turns green; when a feature is removed,
// delete its line.

/// (feature, setting key or "", command name or "")
const ADVERTISED_FEATURES: &[(&str, &str, &str)] = &[
    (
        "spoken emoji (EW parity, brain 2026-09-25 port item 3)",
        "spokenEmoji",
        "",
    ),
    (
        "spoken punctuation (EW parity, brain 2026-09-25 port item 3)",
        "spokenPunctuation",
        "",
    ),
    (
        "unload model after idle (EW parity, brain 2026-09-25 port item 5)",
        "modelUnloadDelaySecs",
        "",
    ),
    (
        "VAD auto-stop (EW parity, brain 2026-09-25 port item 2)",
        "vadAutoStop",
        "",
    ),
    ("live preview (roadmap T1.2)", "livePreviewEnabled", ""),
    (
        "language auto-detect (EW parity, brain 2026-09-25 port item 1)",
        "language",
        "",
    ),
    (
        "engine picker (EW parity, brain 2026-09-25 port item 4)",
        "",
        "select_speech_model",
    ),
    (
        "per-app language overrides (P3.3)",
        "",
        "set_app_language_override",
    ),
];

fn settings_field_names() -> std::collections::BTreeSet<String> {
    let rs = read("crates/teletype-desktop/src/commands.rs");
    let (_, fields) = rust_struct_fields(&rs, "Settings");
    fields.into_iter().map(|(_, k)| k).collect()
}

#[test]
fn contract_09_advertised_features_have_an_engine_path() {
    let settings = settings_field_names();
    let commands = backend_command_names();
    let mut missing: Vec<String> = Vec::new();
    for (feature, setting, command) in ADVERTISED_FEATURES {
        let has_setting = !setting.is_empty() && settings.contains(*setting);
        let has_command = !command.is_empty() && commands.contains(*command);
        if !has_setting && !has_command {
            missing.push(format!(
                "{feature}: no `Settings` field `{setting}` and no \
                 #[tauri::command] `{command}`"
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "advertised feature(s) with no engine path (BUG-002/BUG-003 class — \
         the brain/README promises them, the code has nothing):\n  {missing:#?}\n\
         Build the feature (or remove the advertisement), then this test goes \
         green. Do not weaken the assertion."
    );
}

// ---------------------------------------------------------------------------
// 9. BUG-019 (part 4): nested IPC shapes match on both sides
// ---------------------------------------------------------------------------
//
// contract_03 diffs field NAMES and casing only. A `BTreeMap<String, u32>`
// field serialises to a JS object; if the UI types it as an array (or calls
// array methods on it) tsc cannot see the mismatch. BUG-017 is exactly that:
// `UsageStats.fillerCounts` is an object on the wire but the UI types and
// consumes it as one while the documented expectation is a sorted array.
// This test pins the shape of every container field (Vec/Option/BTreeMap) in
// the curated wire types and fails if the TS side declares a different shape.

/// (rust file, struct, ts file, ts interface)
const WIRE_TYPES_NESTED: &[(&str, &str, &str, &str)] = &[
    (
        "crates/teletype-core/src/transforms/mod.rs",
        "TransformDefinition",
        "ui/src/screens/TransformsScreen.tsx",
        "Transform",
    ),
    (
        "crates/teletype-core/src/personalization/mod.rs",
        "Preference",
        "ui/src/screens/PersonalizationScreen.tsx",
        "Preference",
    ),
    (
        "crates/teletype-core/src/personalization/mod.rs",
        "UserProfile",
        "ui/src/screens/PersonalizationScreen.tsx",
        "Profile",
    ),
    (
        "crates/teletype-core/src/autotext/mod.rs",
        "AutoTextEntry",
        "ui/src/screens/AutoTextScreen.tsx",
        "AutoTextEntry",
    ),
    (
        "crates/teletype-core/src/dictionary.rs",
        "DictionaryWord",
        "ui/src/screens/DictionaryScreen.tsx",
        "DictionaryWord",
    ),
    (
        "crates/teletype-core/src/style.rs",
        "StyleProfile",
        "ui/src/screens/StylesScreen.tsx",
        "StyleProfile",
    ),
    (
        "crates/teletype-desktop/src/commands.rs",
        "HistoryEntry",
        "ui/src/screens/DictationScreen.tsx",
        "HistoryEntry",
    ),
    (
        "crates/teletype-core/src/scratchpad.rs",
        "ScratchEntry",
        "ui/src/screens/ScratchpadScreen.tsx",
        "ScratchEntry",
    ),
    (
        "crates/teletype-core/src/insights.rs",
        "Insights",
        "ui/src/screens/InsightsScreen.tsx",
        "Insights",
    ),
    (
        "crates/teletype-core/src/insights.rs",
        "RankedItem",
        "ui/src/screens/InsightsScreen.tsx",
        "RankedItem",
    ),
    (
        "crates/teletype-core/src/insights.rs",
        "DayStat",
        "ui/src/screens/InsightsScreen.tsx",
        "DayStat",
    ),
    (
        "crates/teletype-core/src/insights.rs",
        "HeatCell",
        "ui/src/screens/InsightsScreen.tsx",
        "HeatCell",
    ),
    (
        "crates/teletype-core/src/insights.rs",
        "Record",
        "ui/src/screens/InsightsScreen.tsx",
        "Record",
    ),
    (
        "crates/teletype-core/src/insights.rs",
        "MilestoneRow",
        "ui/src/screens/InsightsScreen.tsx",
        "MilestoneRow",
    ),
    (
        "crates/teletype-core/src/insights.rs",
        "Milestone",
        "ui/src/screens/InsightsScreen.tsx",
        "Milestone",
    ),
    (
        "crates/teletype-core/src/insights.rs",
        "Impact",
        "ui/src/screens/InsightsScreen.tsx",
        "Impact",
    ),
    (
        "crates/teletype-core/src/usage.rs",
        "UsageStats",
        "ui/src/screens/InsightsScreen.tsx",
        "UsageStats",
    ),
];

/// (rust file, struct, ts file, ts interface)
const NESTED_WIRE_TYPES: &[(&str, &str, &str, &str)] = WIRE_TYPES_NESTED;

fn ts_field_types(src: &str, name: &str) -> std::collections::BTreeMap<String, String> {
    let needle = format!("interface {name} {{");
    let Some(start) = src.find(&needle) else {
        return std::collections::BTreeMap::new();
    };
    let body_start = start + needle.len();
    let Some(rel_end) = src[body_start..].find("\n}") else {
        return std::collections::BTreeMap::new();
    };
    let mut out = std::collections::BTreeMap::new();
    for line in src[body_start..body_start + rel_end].lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("//") || l.starts_with("/**") || l.starts_with("*") {
            continue;
        }
        if let Some((field, ty)) = l.split_once(':') {
            let f = field.trim().trim_end_matches('?').trim().to_string();
            let t = ty.trim().trim_end_matches(';').trim().to_string();
            if !f.is_empty() && f.chars().all(|c| c.is_alphanumeric() || c == '_') {
                out.insert(f, t);
            }
        }
    }
    out
}

/// Classify a TS type as the JS shape it denotes.
fn ts_shape(ty: &str) -> Option<&'static str> {
    let t = ty.trim();
    if t.ends_with("[]") || t.starts_with("Array<") {
        Some("array")
    } else if t.contains("| null") || t.contains("|undefined") || t.starts_with("null") {
        Some("nullable")
    } else if t.starts_with("{") || t.starts_with("Record<") || t.starts_with('[') {
        Some("object")
    } else if t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        // Named interface: look it up lazily by the caller.
        None
    } else {
        Some("scalar")
    }
}

/// Rust type -> expected JS shape.
fn rust_shape(ty: &str) -> Option<&'static str> {
    let t = ty.trim();
    if t == "Vec<String>" || t.starts_with("Vec<") || t.contains("Vec<") {
        Some("array")
    } else if t.starts_with("BTreeMap<") || t.starts_with("HashMap<") {
        Some("object")
    } else if t.starts_with("Option<") {
        Some("nullable")
    } else {
        None
    }
}

#[test]
fn contract_10_nested_ipc_shapes_match() {
    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for (rs_file, rs_struct, ts_file, ts_iface) in NESTED_WIRE_TYPES {
        let rs = read(rs_file);
        let needle = format!("pub struct {rs_struct} {{");
        let Some(start) = rs.find(&needle) else {
            continue;
        };
        let body_start = start + needle.len();
        let Some(end_rel) = rs[body_start..].find("\n}") else {
            continue;
        };
        let body = &rs[body_start..body_start + end_rel];

        // Collect (field, rust type) for container fields only.
        let mut fields: Vec<(String, String)> = Vec::new();
        for line in body.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("pub ") {
                if let Some((fname, ty)) = rest.split_once(':') {
                    let fname = fname.trim();
                    let ty = ty.trim().trim_end_matches(',').trim();
                    if rust_shape(ty).is_some() {
                        fields.push((fname.to_string(), ty.to_string()));
                    }
                }
            }
        }
        if fields.is_empty() {
            continue;
        }
        checked += 1;

        let ts = read(ts_file);
        let types = ts_field_types(&ts, ts_iface);
        for (fname, ty) in &fields {
            let Some(expected) = rust_shape(ty) else {
                continue;
            };
            let key = to_camel(fname);
            let Some(declared) = types.get(&key) else {
                // Absent from the TS interface: contract_03's job, skip.
                continue;
            };
            let actual = match ts_shape(declared) {
                Some(s) => s,
                None => {
                    // Named type: resolve it in the same file.
                    let named = declared.trim();
                    if named.ends_with("[]") {
                        "array"
                    } else {
                        // A declared interface (or a map-like `{ [k: string]: number }`)
                        // serialises to an object on the wire.
                        let _ = ts_field_types(&ts, named);
                        "object"
                    }
                }
            };
            if actual != expected {
                problems.push(format!(
                    "{rs_struct}.{fname} (Rust `{ty}` -> {expected}) is declared as \
                     `{declared}` in {ts_file} interface {ts_iface} ({actual}). The wire \
                     shape and the UI shape disagree; tsc cannot see this. (BUG-017: \
                     fillerCounts was exactly this.)"
                ));
            }
        }
    }

    assert!(
        checked >= 5,
        "only {checked} wire structs with container fields were checked; parser regressed"
    );
    assert!(
        problems.is_empty(),
        "{} nested IPC shape mismatch(es):\n  {problems:#?}\n\nFix the UI type (or the \
         Rust type) so both sides agree, then re-run.",
        problems.len(),
    );
}

// ---------------------------------------------------------------------------
// 10. T5.3: the "Fixes made by Teletype" card reads three data sources over
//     IPC; pin the JSON the UI's arithmetic assumes
// ---------------------------------------------------------------------------
//
// InsightsScreen computes, from the raw IPC payloads:
//   * fillerRemoved              = sum of `usage.fillerCounts` object values
//   * autotextExpansions         = sum of `usage.autotextCounts` object values
//   * personalizationCorrections = sum of `p.count` over
//     `profile.preferences.filter(p => !p.explicit)`
//   * dictionaryLearned          = `dictionary.filter(w => w.learnedFrom != null).length`
// If any of these keys change shape (a u32 that becomes a string, a
// `learnedFrom` that stops being omitted-when-null, a `scope` that stops being
// a bare string for the Global variant) the card silently shows 0 without
// tsc or the shape tests above noticing: those tests only check the
// container-level shape, not the arithmetic inputs. This test serialises the
// same structs the Tauri commands return, the same way the IPC layer does
// (plain `serde_json::to_value`), and recomputes the four numbers the way the
// UI does.

use teletype_core::context::AppType;
use teletype_core::dictionary::{Dictionary, DictionaryWord};
use teletype_core::personalization::{Preference, PreferenceScope, UserProfile};
use teletype_core::usage::UsageStats;

#[test]
fn contract_11_t5_3_fixes_card_data_sources() {
    // --- profile: 2 learned preferences (counts 3 and 5) + 1 explicit
    //     preference (count 9, which the UI must exclude) ---
    let now = 1_700_000_000_000u64;
    let learned_a = Preference {
        id: "learned-a".into(),
        description: "Prefer 'Markets' over 'margets'".into(),
        phrase: "use 'Markets'".into(),
        explicit: false,
        scope: PreferenceScope::Global,
        count: 3,
        created_at: now,
        updated_at: now,
        learned_from: Some("margets".into()),
        learned_to: Some("Markets".into()),
    };
    let learned_b = Preference {
        id: "learned-b".into(),
        description: "Prefer 'IC Markets' in email greetings".into(),
        phrase: "use 'IC Markets'".into(),
        explicit: false,
        scope: PreferenceScope::AppType(AppType::Email),
        count: 5,
        created_at: now,
        updated_at: now,
        learned_from: None,
        learned_to: None,
    };
    let explicit = Preference {
        id: "explicit-1".into(),
        description: "Always sign off 'Regards'".into(),
        phrase: "sign with 'Regards'".into(),
        explicit: true,
        scope: PreferenceScope::Global,
        count: 9,
        created_at: now,
        updated_at: now,
        learned_from: None,
        learned_to: None,
    };
    let profile = UserProfile {
        language: "en".into(),
        preferences: vec![learned_a, learned_b, explicit],
        ..Default::default()
    };
    let profile_json = serde_json::to_value(&profile).expect("UserProfile serialises");

    // The arithmetic inputs must be the JSON types the UI's reduce/filter
    // assume: `explicit` a bool, `count` a number.
    let prefs = profile_json
        .get("preferences")
        .and_then(|v| v.as_array())
        .expect("preferences is a JSON array");
    assert_eq!(prefs.len(), 3);
    for p in prefs {
        assert!(
            p.get("explicit").is_some_and(|v| v.is_boolean()),
            "Preference.explicit must serialise as a JSON bool, got {:?}",
            p.get("explicit")
        );
        assert!(
            p.get("count").is_some_and(|v| v.is_number()),
            "Preference.count must serialise as a JSON number, got {:?}",
            p.get("count")
        );
    }
    // `scope` is an enum: the unit variant must be a bare string ("global")
    // and the tuple variant a single-key object ({"appType": "email"}), both
    // camelCase. A rename that turns the unit variant into an object would
    // not break tsc (the TS type is `string | { appType: string }`) but would
    // be a wire change the UI never asked for.
    let scopes: Vec<&serde_json::Value> = prefs.iter().map(|p| p.get("scope").unwrap()).collect();
    let globals = scopes.iter().filter(|s| s.as_str().is_some()).count();
    let app_types = scopes
        .iter()
        .filter(|s| s.get("appType").and_then(|v| v.as_str()).is_some())
        .count();
    assert_eq!(globals, 2, "Global scope must be a bare string: {scopes:?}");
    assert_eq!(
        app_types, 1,
        "AppType scope must be {{\"appType\": ...}}: {scopes:?}"
    );
    assert_eq!(scopes[0].as_str(), Some("global"));
    assert_eq!(
        scopes[1].get("appType").and_then(|v| v.as_str()),
        Some("email")
    );

    // Recompute the UI's sum from the wire JSON: learned (non-explicit)
    // counts only, 3 + 5 = 8, with the explicit preference's 9 excluded.
    let learned_sum: u64 = prefs
        .iter()
        .filter(|p| p.get("explicit").and_then(|v| v.as_bool()) == Some(false))
        .map(|p| {
            p.get("count")
                .and_then(|v| v.as_u64())
                .expect("count is a number")
        })
        .sum();
    assert_eq!(
        learned_sum, 8,
        "the UI's personalizationCorrections sums count over !explicit: expected 3+5"
    );
    let all_sum: u64 = prefs
        .iter()
        .map(|p| {
            p.get("count")
                .and_then(|v| v.as_u64())
                .expect("count is a number")
        })
        .sum();
    assert_eq!(
        all_sum, 17,
        "sanity: the explicit preference's count of 9 is present (8+9)"
    );

    // --- dictionary: 3 words, exactly one learned (learnedFrom set) ---
    let mut learned_word = DictionaryWord::new("Markets", "");
    learned_word.mark_learned("margets");
    let dict = Dictionary {
        words: vec![
            DictionaryWord::new("Teletype", "tel-uh-type"),
            DictionaryWord::new("IC Markets", ""),
            learned_word,
        ],
        builtin_version: 0,
    };
    let words_json = serde_json::to_value(&dict.words).expect("DictionaryWord serialises");
    let words = words_json.as_array().expect("words is a JSON array");
    assert_eq!(words.len(), 3);
    // The UI counts `w.learnedFrom != null`; with skip_serializing_if the key
    // is absent for non-learned words and present (non-null) for the learned
    // one. Both "absent" and "null" are falsy in TS, but pin the actual
    // behaviour so a future serde change cannot drift silently.
    let learned_count = words
        .iter()
        .filter(|w| w.get("learnedFrom").is_some_and(|v| !v.is_null()))
        .count();
    assert_eq!(
        learned_count, 1,
        "the UI's dictionaryLearned counts learnedFrom != null: expected exactly 1"
    );
    let learned = words
        .iter()
        .find(|w| w.get("learnedFrom").is_some())
        .expect("the learned word carries learnedFrom");
    assert_eq!(
        learned.get("learnedFrom").and_then(|v| v.as_str()),
        Some("margets")
    );
    assert!(
        learned.get("learnedAt").and_then(|v| v.as_u64()).is_some(),
        "learnedAt is set together with learnedFrom (mark_learned)"
    );

    // --- usage stats: known filler/autotext counts, sums match the UI ---
    let mut usage = UsageStats::default();
    usage.filler_counts.insert("um".into(), 4);
    usage.filler_counts.insert("like".into(), 7);
    usage.autotext_counts.insert("/email".into(), 6);
    usage.autotext_counts.insert("full stop".into(), 2);
    let usage_json = serde_json::to_value(&usage).expect("UsageStats serialises");
    let filler: u64 = usage_json
        .get("fillerCounts")
        .and_then(|v| v.as_object())
        .expect("fillerCounts is a JSON object")
        .values()
        .map(|v| v.as_u64().expect("filler count is a number"))
        .sum();
    let autotext: u64 = usage_json
        .get("autotextCounts")
        .and_then(|v| v.as_object())
        .expect("autotextCounts is a JSON object")
        .values()
        .map(|v| v.as_u64().expect("autotext count is a number"))
        .sum();
    assert_eq!(
        filler, 11,
        "the UI's fillerRemoved sums fillerCounts values: 4+7"
    );
    assert_eq!(
        autotext, 8,
        "the UI's autotextExpansions sums autotextCounts values: 6+2"
    );
}
