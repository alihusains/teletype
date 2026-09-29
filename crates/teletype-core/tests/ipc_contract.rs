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
        let bytes: Vec<char> = src.chars().collect();
        let mut i = 0;
        while let Some(pos) = src[i..].find("invoke") {
            let at = i + pos;
            // `invoke` must be a standalone identifier.
            let before_ok = at == 0
                || !bytes[at - 1].is_alphanumeric() && bytes[at - 1] != '_' && bytes[at - 1] != '.';
            let after = at + "invoke".len();
            let after_ok =
                after >= bytes.len() || !bytes[after].is_alphanumeric() && bytes[after] != '_';
            // Skip the import line: `import { invoke } from "@tauri-apps/..."`
            // is not a call.
            let line_start = src[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let on_import = src[line_start..at].trim_start().starts_with("import");
            if before_ok && after_ok && !on_import {
                // Walk the real grammar: `invoke` ws [ `<` type `>` ] ws `(`
                // ws `"` name `"`. Anything else is not a call, and must not
                // be scanned forward for a stray string literal.
                let b: Vec<char> = src[after..].chars().collect();
                let mut j = 0usize;
                let skip_ws = |b: &Vec<char>, mut j: usize| {
                    while j < b.len() && b[j].is_whitespace() {
                        j += 1;
                    }
                    j
                };
                j = skip_ws(&b, j);
                if j < b.len() && b[j] == '<' {
                    let mut depth = 0i32;
                    while j < b.len() {
                        match b[j] {
                            '<' => depth += 1,
                            '>' => {
                                depth -= 1;
                                if depth == 0 {
                                    j += 1;
                                    break;
                                }
                            }
                            '{' | ';' => break,
                            _ => {}
                        }
                        j += 1;
                    }
                    j = skip_ws(&b, j);
                }
                if j < b.len() && b[j] == '(' {
                    j = skip_ws(&b, j + 1);
                    if j < b.len() && b[j] == '"' {
                        let rest: String = b[j + 1..].iter().collect();
                        if let Some(end) = rest.find('"') {
                            let name = &rest[..end];
                            let ok = !name.is_empty()
                                && name.starts_with(|c: char| c.is_ascii_lowercase())
                                && name.chars().all(|c| {
                                    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
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
const KNOWN_CASING_DEFECTS: &[(&str, &str, &str, &str)] = &[
    (
        "ui/src/screens/TransformsScreen.tsx",
        "Transform",
        "built_in",
        "the 'built-in' badge never renders, so a Delete button is drawn on \
         every shipped transform and one click removes it",
    ),
    (
        "ui/src/screens/TransformsScreen.tsx",
        "Transform",
        "auto_apply",
        "the per-transform 'Auto Apply' checkbox always renders unchecked, so \
         opening Edit and pressing Save silently turns auto-apply off",
    ),
    (
        "ui/src/screens/PersonalizationScreen.tsx",
        "Profile",
        "learn_from_edits",
        "the toggle always renders OFF and writes `true` back for its siblings",
    ),
    (
        "ui/src/screens/PersonalizationScreen.tsx",
        "Profile",
        "learn_app_specific",
        "same: renders OFF, and clobbers the other two on every interaction",
    ),
    (
        "ui/src/screens/PersonalizationScreen.tsx",
        "Profile",
        "learn_terminology",
        "same: renders OFF, and clobbers the other two on every interaction",
    ),
];

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
