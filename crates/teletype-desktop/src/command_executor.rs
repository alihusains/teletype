//! Command executor: turns a structured `CommandAction` into a real
//! system action (open an app, set brightness, mute the speaker, etc.).
//!
//! Each action is implemented with the simplest platform API available
//! on macOS. Actions that require confirmation (destructive operations)
//! are not executed here — the caller must check the risk level first.
//!
//! P2.7: arguments are treated as untrusted. Commands can be produced by
//! the LLM fallback (`teletype_inference::command_llm`) or transcribed from
//! audio, so every string argument is validated before any tool runs — see
//! [`reject_control_characters`] and [`apple_script_escape`].
//!
//! W1: side effects are routed through an [`Effects`] seam so a
//! `DryRunEffects` can record what *would* happen without spawning a
//! process, touching the clipboard, or pressing a key. The real
//! `SystemEffects` dispatches to the same handler functions the pre-W1
//! `execute` used to call directly, so existing behavior is unchanged.
//!
//! W1 (evidence): `send_message` no longer trusts the `osascript` exit
//! code as proof the message left. The outcome is modeled as
//! [`SendEvidence`] and formatted into one of three result strings:
//! verified, unverified, or a hard `Err` when the send itself failed.

use std::sync::{Mutex, OnceLock};

use teletype_core::context::ApplicationContext;
use teletype_core::llm::CommandAction;

/// Executes a `CommandAction` against the real system and returns a
/// human-readable result.
///
/// Equivalent to `execute_with(&SystemEffects, cmd, ctx)`. Kept with the
/// exact pre-W1 signature so existing call sites (dictation pipeline) keep
/// working.
pub fn execute(cmd: &CommandAction, context: &ApplicationContext) -> Result<String, String> {
    execute_with(&SystemEffects, cmd, context)
}

/// Executes a `CommandAction` against the given effects backend.
///
/// Validation (control characters, tool allowlist) runs *before* effects,
/// so a dry-run never records an unvalidated command.
pub fn execute_with(
    effects: &dyn Effects,
    cmd: &CommandAction,
    context: &ApplicationContext,
) -> Result<String, String> {
    reject_control_characters(cmd)?;
    let args = args_of(cmd);
    match cmd.tool.as_str() {
        "open_app" => effects.run("open_app", args, context),
        "open_url" => effects.run("open_url", args, context),
        "set_brightness" => effects.run("set_brightness", args, context),
        "set_volume" => effects.run("set_volume", args, context),
        "mute_speaker" => effects.run("mute_speaker", args, context),
        "keyboard_shortcut" => effects.run("keyboard_shortcut", args, context),
        "open_settings" => effects.run("open_settings", args, context),
        "play_media" => effects.run("play_media", args, context),
        "set_timer" => effects.run("set_timer", args, context),
        "send_message" => effects.run("send_message", args, context),
        "compose_email" => effects.run("compose_email", args, context),
        "create_calendar_event" => effects.run("create_calendar_event", args, context),
        "open_file" => effects.run("open_file", args, context),
        "type_text" => effects.run("type_text", args, context),
        other => Err(format!("unknown command tool: {other}")),
    }
}

/// Returns the argument object of a `CommandAction`, or a shared empty
/// map when `arguments` is not a JSON object (defensive; the fallback
/// parser always emits an object, but a hand-built value could be
/// anything).
///
/// The empty-map fallback is a lazily-initialized `static` returned
/// through a function (not a `const`), which avoids the clippy
/// "named constant with interior mutability" lint that a
/// `const LazyLock<...>` triggers.
fn args_of(cmd: &CommandAction) -> &serde_json::Map<String, serde_json::Value> {
    match cmd.arguments.as_object() {
        Some(m) => m,
        None => empty_args(),
    }
}

fn empty_args() -> &'static serde_json::Map<String, serde_json::Value> {
    static EMPTY: std::sync::OnceLock<serde_json::Map<String, serde_json::Value>> =
        std::sync::OnceLock::new();
    EMPTY.get_or_init(serde_json::Map::new)
}

/// The seam between tool dispatch and the side effects a tool performs.
///
/// Two implementations:
/// - [`SystemEffects`] — the real thing: spawns `open`/`osascript`, touches
///   the clipboard, presses keys.
/// - [`DryRunEffects`] — records `"would <tool> <args-json>"` and returns
///   `Ok` without any OS interaction, so a dry-run test path is possible.
///
/// `Send + Sync` so a single backend can be shared across the pipeline
/// worker threads that call [`execute`].
pub trait Effects: Send + Sync {
    /// Run one tool and return the human-readable result string.
    fn run(
        &self,
        tool: &str,
        args: &serde_json::Map<String, serde_json::Value>,
        ctx: &ApplicationContext,
    ) -> Result<String, String>;
}

/// The production effects backend. Dispatches to the same handler
/// functions the pre-W1 `execute` matched on, so the observable behavior
/// for the ten original tools is unchanged (except `send_message`, which
/// now reports evidence, per the W1 contract).
pub struct SystemEffects;

/// A no-side-effects backend for tests and dry-runs. Records every
/// dispatched tool as one line (`"would <tool> <args-json>"`) behind a
/// `Mutex` so a shared instance can be read from multiple threads.
#[cfg_attr(not(test), allow(dead_code))]
pub struct DryRunEffects {
    recorded: Mutex<Vec<String>>,
}

impl Default for DryRunEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl DryRunEffects {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new() -> Self {
        Self {
            recorded: Mutex::new(Vec::new()),
        }
    }

    /// Returns a snapshot of every `"would …"` line recorded so far, in
    /// dispatch order. Exposed for tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn recorded(&self) -> Vec<String> {
        self.recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Effects for DryRunEffects {
    fn run(
        &self,
        tool: &str,
        args: &serde_json::Map<String, serde_json::Value>,
        _ctx: &ApplicationContext,
    ) -> Result<String, String> {
        // Serialize once; the recorded line doubles as the returned
        // string so a test can assert on both.
        let line = format!(
            "would {tool} {}",
            serde_json::to_string(args).unwrap_or_default()
        );
        self.recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(line.clone());
        Ok(line)
    }
}

impl Effects for SystemEffects {
    fn run(
        &self,
        tool: &str,
        args: &serde_json::Map<String, serde_json::Value>,
        ctx: &ApplicationContext,
    ) -> Result<String, String> {
        // Rebuild a `CommandAction` so the existing per-tool handlers
        // (which read via `arg_str(cmd, key)`) work unchanged.
        let cmd = CommandAction {
            tool: tool.to_string(),
            arguments: serde_json::Value::Object(args.clone()),
        };
        match tool {
            "open_app" => execute_open_app(&cmd),
            "open_url" => execute_open_url(&cmd),
            "set_brightness" => execute_set_brightness(&cmd),
            "set_volume" => execute_set_volume(&cmd),
            "mute_speaker" => execute_mute_speaker(&cmd),
            "keyboard_shortcut" => execute_keyboard_shortcut(&cmd),
            "open_settings" => execute_open_settings(&cmd),
            "play_media" => execute_play_media(&cmd),
            "set_timer" => execute_set_timer(&cmd),
            "send_message" => execute_send_message(&cmd),
            "compose_email" => execute_compose_email(&cmd),
            "create_calendar_event" => execute_create_calendar_event(&cmd),
            "open_file" => execute_open_file(&cmd),
            "type_text" => execute_type_text(&cmd, ctx),
            other => Err(format!("unknown command tool: {other}")),
        }
    }
}

/// Rejects string arguments containing control characters.
///
/// A `\n` inside `send_message`'s `to`/`text` would end the AppleScript
/// statement and start a new one (the classic script injection); the same
/// guard covers every other tool for free. Unknown argument types (numbers,
/// booleans) pass through — tools re-validate their own fields.
///
/// W1 exception: `\n` and `\t` are legitimate inside `type_text.text` and
/// `compose_email.body` — neither path goes through AppleScript, and a
/// multi-line dictation is the whole point of `type_text`.
fn reject_control_characters(cmd: &CommandAction) -> Result<(), String> {
    if let Some(args) = cmd.arguments.as_object() {
        for (key, value) in args {
            if let Some(s) = value.as_str() {
                if s.chars().any(char::is_control) {
                    // Per-tool exception: allow \n and \t in the two fields
                    // where they are legitimate content, not injection.
                    if is_allowed_control_field(cmd.tool.as_str(), key)
                        && s.chars().all(|c| c == '\n' || c == '\t' || !c.is_control())
                    {
                        continue;
                    }
                    return Err(format!(
                        "{}: '{key}' contains control characters and was rejected",
                        cmd.tool
                    ));
                }
            }
        }
    }
    Ok(())
}

/// True for the (tool, field) pairs where `\n`/`\t` are legitimate content.
/// Every other control character is rejected in these fields too; this
/// only carves out the two line-terminating characters.
fn is_allowed_control_field(tool: &str, field: &str) -> bool {
    matches!(
        (tool, field),
        ("type_text", "text") | ("compose_email", "body")
    )
}

/// AppleScript string-literal escaping: backslash first, then quote.
///
/// Order matters. Escaping quotes first turns `\"` (backslash + quote) into
/// `\\"` — an escaped backslash followed by a *raw* quote — which ends the
/// string literal early. That sequence is why the single naive `replace`
/// this helper grew out of was not enough: a crafted (or hallucinated)
/// value could still inject AppleScript.
fn apple_script_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The Messages script for one send. Pure, so the escaping is testable
/// without running `osascript`.
///
/// macOS 27 removed the `participants` element from the `account` class, so
/// the old `participant "<to>" of targetService` form no longer compiles.
/// The application-level `participant "<handle>"` form does work, and a live
/// self-send through it delivered (2026-10-27). It resolves by handle only:
/// passing a contact name creates a participant named after the literal
/// string, so callers resolve names through [`resolve_contact_handle`] first.
fn messages_script(to: &str, text: &str) -> String {
    format!(
        "tell application \"Messages\"\nactivate\nsend \"{}\" to participant \"{}\"\nend tell",
        apple_script_escape(text),
        apple_script_escape(to)
    )
}

/// True when `to` can be an iMessage handle: an email address, a `+` number,
/// or a bare number (spaces, dashes, parentheses allowed). Anything else is
/// treated as a spoken contact name and resolved through Contacts first.
fn looks_like_handle(to: &str) -> bool {
    let t = to.trim();
    let bare = t
        .strip_prefix("e:")
        .or_else(|| t.strip_prefix("p:"))
        .unwrap_or(t);
    !bare.is_empty()
        && (bare.contains('@')
            || (bare.starts_with('+') && bare.chars().any(|c| c.is_ascii_digit()))
            || bare
                .chars()
                .all(|c| c.is_ascii_digit() || " -()".contains(c)))
}

/// One phone or email entry on a Contacts card.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ContactHandle {
    kind: ContactKind,
    /// Raw Contacts label, e.g. `_$!<Mobile>!$_` or `_$!<Work>!$_`.
    label: String,
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContactKind {
    Phone,
    Email,
}

/// Pure outcome of a Contacts lookup, before it becomes an error string.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ContactLookup {
    Found(String),
    NotFound,
    /// More than one matching card carries a usable phone or email.
    Ambiguous(usize),
    /// The name matched, but no card has a usable phone or email.
    NoHandle,
}

/// The Contacts script that dumps every card whose full name is `name` as
/// `PERSON`, then `PHONE <label> <value>` / `EMAIL <label> <value>` lines.
///
/// `label` is not guaranteed to exist on every entry, so it is read inside a
/// `try`; a missing value skips that entry rather than failing the lookup.
/// The name is escaped like every other AppleScript string literal.
fn contacts_lookup_script(name: &str) -> String {
    format!(
        r#"tell application "Contacts"
set out to ""
set matches to (every person whose name is "{name}")
repeat with p in matches
set out to out & "PERSON" & linefeed
repeat with ph in phones of p
set lbl to ""
try
set lbl to (label of ph as text)
end try
try
set v to (value of ph as text)
set out to out & "PHONE" & tab & lbl & tab & v & linefeed
end try
end repeat
repeat with em in emails of p
set lbl to ""
try
set lbl to (label of em as text)
end try
try
set v to (value of em as text)
set out to out & "EMAIL" & tab & lbl & tab & v & linefeed
end try
end repeat
end repeat
return out
end tell"#,
        name = apple_script_escape(name)
    )
}

/// Strips formatting from a phone value so Messages sees the canonical
/// handle: `+1 (555) 222-3333` becomes `+15552223333`. Numbers without a
/// leading `+` keep their digits only. Returns an empty string when the
/// value has no digits, which the parser then drops.
fn normalize_phone(value: &str) -> String {
    let trimmed = value.trim();
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('+') {
        format!("+{digits}")
    } else {
        digits
    }
}

/// Picks the best handle on one card: mobile/iPhone phone first (where
/// iMessage usually lands), then email, then any remaining phone.
fn pick_contact_handle(handles: &[ContactHandle]) -> Option<String> {
    let is_mobile = |h: &ContactHandle| {
        h.kind == ContactKind::Phone && {
            let label = h.label.to_ascii_lowercase();
            label.contains("mobile") || label.contains("iphone")
        }
    };
    handles
        .iter()
        .find(|h| is_mobile(h))
        .or_else(|| handles.iter().find(|h| h.kind == ContactKind::Email))
        .or_else(|| handles.iter().find(|h| h.kind == ContactKind::Phone))
        .map(|h| h.value.clone())
}

/// Parses [`contacts_lookup_script`] output. Cards without any usable
/// handle are ignored: a name that matches one card with an email and one
/// empty card is a unique match, not an ambiguity.
fn parse_contact_lookup(raw: &str) -> ContactLookup {
    let mut cards: Vec<Vec<ContactHandle>> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "PERSON" {
            cards.push(Vec::new());
            continue;
        }
        let Some(card) = cards.last_mut() else {
            continue;
        };
        let mut parts = line.splitn(3, '\t');
        let kind = match parts.next() {
            Some("PHONE") => ContactKind::Phone,
            Some("EMAIL") => ContactKind::Email,
            _ => continue,
        };
        let label = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if value.is_empty() {
            continue;
        }
        let value = match kind {
            ContactKind::Phone => normalize_phone(value),
            ContactKind::Email => value.to_string(),
        };
        if !looks_like_handle(&value) {
            continue;
        }
        card.push(ContactHandle {
            kind,
            label: label.to_string(),
            value,
        });
    }

    let with_handles: Vec<&Vec<ContactHandle>> =
        cards.iter().filter(|card| !card.is_empty()).collect();
    match with_handles.len() {
        0 if cards.is_empty() => ContactLookup::NotFound,
        0 => ContactLookup::NoHandle,
        1 => match pick_contact_handle(with_handles[0]) {
            Some(handle) => ContactLookup::Found(handle),
            None => ContactLookup::NoHandle,
        },
        n => ContactLookup::Ambiguous(n),
    }
}

/// Maps a Contacts AppleScript failure to the tool error, adding the privacy
/// hint only when access was denied.
#[cfg(target_os = "macos")]
fn contacts_lookup_error(err: &str) -> String {
    let hint = if err.contains("(-1743)") || err.contains("Not authorized") {
        " (grant Teletype access to Contacts in System Settings > Privacy & Security > Contacts)"
    } else {
        ""
    };
    format!("send_message: Contacts lookup failed: {err}{hint}")
}

/// Resolves a spoken contact name to an iMessage handle through Contacts.
/// macOS Messages scripting only understands handles, so this is the step
/// that makes "text Sara" work without the model guessing an address.
#[cfg(target_os = "macos")]
fn resolve_contact_handle(name: &str) -> Result<String, String> {
    let script = contacts_lookup_script(name);
    let raw = match run_osascript(&script) {
        Ok(raw) => raw,
        // Measured 2026-10-27: Contacts does not always auto-launch from a
        // `tell` block (`launch` itself returns -600). Opening the app
        // explicitly recovers the lookup.
        Err(first) if first.contains("(-600)") => {
            let _ = std::process::Command::new("open")
                .args(["-a", "Contacts"])
                .status();
            std::thread::sleep(std::time::Duration::from_millis(800));
            match run_osascript(&script) {
                Ok(raw) => raw,
                Err(err) => return Err(contacts_lookup_error(&err)),
            }
        }
        Err(err) => return Err(contacts_lookup_error(&err)),
    };
    match parse_contact_lookup(&raw) {
        ContactLookup::Found(handle) => Ok(handle),
        ContactLookup::NotFound => Err(format!(
            "send_message: no contact named {name:?}; say a phone number or email instead"
        )),
        ContactLookup::Ambiguous(n) => Err(format!(
            "send_message: {n} contacts named {name:?}; say the phone number or email instead"
        )),
        ContactLookup::NoHandle => Err(format!(
            "send_message: contact {name:?} has no phone number or email"
        )),
    }
}

/// The evidence script: re-reads the newest message in the conversation
/// with `to` and prints its text as a single line (no labels, no extra
/// whitespace).
///
/// Design notes:
/// - The script is *pure* (no side effects) — it only reads.
/// - It fails cleanly (non-zero exit or empty output) when the Messages
///   AppleScript API on a given macOS version cannot expose conversation
///   history. The Rust side treats "empty" and "non-zero" identically:
///   evidence is unavailable, so the send is reported as *unverified*,
///   never *sent*.
/// - `to` is interpolated the same way as in [`messages_script`] (escaped
///   string literal); the control-character guard already rejected a
///   newline in it.
fn messages_evidence_script(to: &str) -> String {
    format!(
        "tell application \"Messages\"\n\
             set targetService to 1st account whose service type = iMessage service type\n\
             set targetBuddy to participant \"{}\" of targetService\n\
             set convs to (every conversation of targetService whose 1 participant = targetBuddy)\n\
             if (count of convs) is 0 then\n\
                 return \"\"\n\
             end if\n\
             set latestMsg to (last message of (item 1 of convs))\n\
             return (content of latestMsg as text)\n\
             end tell",
        apple_script_escape(to)
    )
}

/// The explicit outcome of a `send_message` call after the evidence read.
///
/// Modeled as an enum so the formatter is a pure function of the outcome
/// — no I/O, no clock, no process. The retry loop (up to 2 reads, ~150 ms
/// apart) produces one of these three values; the handler formats it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendEvidence {
    /// The evidence read returned a message whose text (trim + collapse
    /// whitespace, case-sensitive compare) matches the sent text.
    Verified,
    /// The send command succeeded but the evidence was unavailable (script
    /// failed, empty output, or the read timed out) or did not match.
    /// The `String` is a short, user-visible reason.
    Unverified(String),
    /// The send itself failed (the `osascript` exit status was non-zero,
    /// or `osascript` could not be spawned). The `String` is the error.
    #[cfg_attr(not(test), allow(dead_code))]
    Failed(String),
}

/// Compares two strings after trimming and collapsing internal runs of
/// whitespace to single spaces. Case-sensitive. Used by
/// [`parse_evidence_output`] and by the send handler.
fn normalized_eq(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    norm(a) == norm(b)
}

/// Parses the raw `osascript` output of [`messages_evidence_script`] into a
/// [`SendEvidence`], given the text that was sent.
///
/// Pure. Unit-tested with fake script output; never runs `osascript`.
///
/// Decision table:
/// - `raw` empty (after trim) → `Unverified("evidence read returned no text")`
/// - `raw` (normalized) == `sent` (normalized) → `Verified`
/// - otherwise → `Unverified("evidence text did not match")`
///
/// A *failed* script (non-zero exit) is represented on the caller side by
/// passing an empty `raw` (the handler only calls this function on
/// success of the evidence read); the handler itself produces
/// `Unverified("evidence read failed")` on a non-zero exit.
pub fn parse_evidence_output(raw: &str, sent: &str) -> SendEvidence {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return SendEvidence::Unverified("evidence read returned no text".into());
    }
    if normalized_eq(trimmed, sent) {
        SendEvidence::Verified
    } else {
        SendEvidence::Unverified("evidence text did not match".into())
    }
}

/// Formats a [`SendEvidence`] into the user-visible result string for
/// `send_message`.
///
/// - `Verified` → `"sent message to <to> (verified)"`
/// - `Unverified(reason)` → `"sent message to <to> (unverified: <reason>)"`
/// - `Failed(reason)` → the caller maps this to `Err(reason)`; this
///   function returns `Ok` only for the two success-shaped outcomes, and
///   `Err` for `Failed` (kept here so the handler stays one-liner).
pub fn format_send_evidence(to: &str, evidence: &SendEvidence) -> Result<String, String> {
    match evidence {
        SendEvidence::Verified => Ok(format!("sent message to {to} (verified)")),
        SendEvidence::Unverified(reason) => {
            Ok(format!("sent message to {to} (unverified: {reason})"))
        }
        SendEvidence::Failed(reason) => Err(reason.clone()),
    }
}

fn arg_str(cmd: &CommandAction, key: &str) -> Option<String> {
    cmd.arguments
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn arg_bool(cmd: &CommandAction, key: &str) -> Option<bool> {
    cmd.arguments.get(key).and_then(|v| v.as_bool())
}

fn execute_open_app(cmd: &CommandAction) -> Result<String, String> {
    let app = arg_str(cmd, "app").ok_or("open_app: missing 'app' argument")?;
    let url = arg_str(cmd, "url");

    #[cfg(target_os = "macos")]
    {
        let app_name = resolve_app_name(&app);
        if let Some(ref url) = url {
            // Open the app with a URL (e.g. "open brave" + "slack.com").
            let status = std::process::Command::new("open")
                .args(["-a", &app_name, url])
                .status()
                .map_err(|e| format!("open -a {app_name} {url}: {e}"))?;
            if status.success() {
                Ok(format!("opened {app_name} with {url}"))
            } else {
                Err(format!("open -a {app_name} {url} failed: {status}"))
            }
        } else {
            let status = std::process::Command::new("open")
                .args(["-a", &app_name])
                .status()
                .map_err(|e| format!("open -a {app_name}: {e}"))?;
            if status.success() {
                Ok(format!("opened {app_name}"))
            } else {
                Err(format!("open -a {app_name} failed: {status}"))
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, url);
        Err("open_app not supported on this platform".into())
    }
}

/// Maps spoken app names to macOS application names.
/// First checks a hardcoded list of common apps, then falls back to
/// fuzzy-matching against installed applications in /Applications.
fn resolve_app_name(app: &str) -> String {
    // Hardcoded aliases for common apps.
    match app.to_lowercase().as_str() {
        "vs code" | "visual studio code" | "vscode" | "code" => return "Visual Studio Code".into(),
        "app store" => return "App Store".into(),
        "file manager" | "finder" => return "Finder".into(),
        "terminal" | "command line" | "console" => return "Terminal".into(),
        "system settings" | "settings" | "preferences" => return "System Settings".into(),
        "screenshot" => return "Screenshot".into(),
        _ => {}
    }

    // Fuzzy-match against installed applications.
    #[cfg(target_os = "macos")]
    {
        if let Some(matched) = fuzzy_match_installed_app(app) {
            return matched;
        }
    }

    // Fallback: capitalize the first letter.
    let mut chars = app.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Scans the standard macOS application directories for .app bundles and
/// fuzzy-matches the query against their names. Returns the best match if
/// the similarity score is above a threshold.
///
/// This is the dynamic half of app resolution: whatever the user says
/// ("terax", "kalkulyator", "brave") is matched against the apps that are
/// actually installed, so a mis-heard name still lands on the right app.
#[cfg(target_os = "macos")]
fn fuzzy_match_installed_app(query: &str) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = [
        "/Applications",
        "/System/Applications",
        "/System/Applications/Utilities",
        &format!("{home}/Applications"),
    ];

    // Strip a leading "open"/"op"/"launch"/"start" verb that the STT may
    // have captured as part of the app name (e.g. "op visper flo" → "visper flo").
    let query_lower = strip_open_verb(query.to_lowercase());
    let mut best_match: Option<(String, f64)> = None;
    const THRESHOLD: f64 = 0.5; // minimum similarity to accept a match

    for dir in &dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("app") {
                continue;
            }
            let app_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let app_lower = app_name.to_lowercase();

            // Exact match (case-insensitive) wins immediately.
            if app_lower == query_lower {
                return Some(app_name);
            }

            // One name contains the other (e.g. "brave" in "brave browser",
            // or "terminal" in "terminal"). Strong match.
            if app_lower.contains(&query_lower) || query_lower.contains(&app_lower) {
                let score = 0.9;
                if best_match.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                    best_match = Some((app_name, score));
                }
                continue;
            }

            // Word-boundary boost: if the query and app name share a word
            // (e.g. "whisper flow" and "wispr flow" both end in "flow"),
            // boost the Levenshtein score. This handles multi-word brand
            // names where the STT mangles one word but preserves another.
            let base_sim = levenshtein_similarity(&query_lower, &app_lower);
            let shared_word = shares_word(&query_lower, &app_lower);
            let similarity = if shared_word {
                (base_sim + 1.0) / 2.0 // average with a perfect score
            } else {
                base_sim
            };
            if similarity >= THRESHOLD
                && best_match
                    .as_ref()
                    .map(|(name, s)| similarity > *s && name != &query_lower)
                    .unwrap_or(true)
            {
                best_match = Some((app_name, similarity));
            }
        }
    }

    best_match.map(|(name, _)| name)
}

/// Strips a leading "open", "op", "launch", or "start" verb (and any
/// following space) from a lowercased query.
fn strip_open_verb(q: String) -> String {
    for verb in ["open ", "op ", "launch ", "start "] {
        if let Some(rest) = q.strip_prefix(verb) {
            return rest.trim().to_string();
        }
    }
    q
}

/// Returns true if the query and app name share at least one word of
/// 3+ characters (case-insensitive). Handles multi-word brand names where
/// the STT mangles one word but preserves another (e.g. "whisper flow"
/// and "wispr flow" both contain "flow").
fn shares_word(query: &str, app: &str) -> bool {
    let q_words: Vec<&str> = query.split_whitespace().collect();
    let a_words: Vec<&str> = app.split_whitespace().collect();
    q_words
        .iter()
        .any(|qw| qw.len() >= 3 && a_words.iter().any(|aw| aw == qw))
}

/// Computes a similarity score between two strings using normalized
/// Levenshtein distance. Returns 1.0 for identical strings, 0.0 for
/// completely different strings.
fn levenshtein_similarity(a: &str, b: &str) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let a_len = a.len();
    let b_len = b.len();
    let max_len = a_len.max(b_len);
    if max_len == 0 {
        return 1.0;
    }

    // Compute Levenshtein distance.
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a.chars().nth(i - 1) == b.chars().nth(j - 1) {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    let distance = prev[b_len] as f64;
    1.0 - (distance / max_len as f64)
}

fn execute_open_url(cmd: &CommandAction) -> Result<String, String> {
    let url = arg_str(cmd, "url").ok_or("open_url: missing 'url' argument")?;
    let full_url = if url.contains("://") {
        url
    } else if url.contains('.') && !url.contains(' ') {
        format!("https://{url}")
    } else {
        // Search query, not a URL.
        format!("https://www.google.com/search?q={}", url.replace(' ', "+"))
    };

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg(&full_url)
            .status()
            .map_err(|e| format!("open {full_url}: {e}"))?;
        if status.success() {
            Ok(format!("opened {full_url}"))
        } else {
            Err(format!("open {full_url} failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = full_url;
        Err("open_url not supported on this platform".into())
    }
}

fn execute_set_brightness(cmd: &CommandAction) -> Result<String, String> {
    let level = arg_str(cmd, "level").ok_or("set_brightness: missing 'level' argument")?;

    #[cfg(target_os = "macos")]
    {
        // Use AppleScript to set brightness.
        let script = match level.as_str() {
            "higher" | "increase" => "tell application \"System Events\" to key code 149", // F2 (brightness up)
            "lower" | "decrease" => "tell application \"System Events\" to key code 145", // F1 (brightness down)
            "max" | "100" => "tell application \"System Events\" to key code 149", // press F2 multiple times
            "min" | "0" => "tell application \"System Events\" to key code 145", // press F1 multiple times
            _ => {
                // Percentage: press the appropriate key N times.
                let percent: u32 = level.parse().unwrap_or(50);
                if percent > 50 {
                    "tell application \"System Events\" to key code 149"
                } else {
                    "tell application \"System Events\" to key code 145"
                }
            }
        };
        let status = std::process::Command::new("osascript")
            .args(["-e", script])
            .status()
            .map_err(|e| format!("osascript brightness: {e}"))?;
        if status.success() {
            Ok(format!("brightness set to {level}"))
        } else {
            Err(format!("brightness set to {level} failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = level;
        Err("set_brightness not supported on this platform".into())
    }
}

fn execute_set_volume(cmd: &CommandAction) -> Result<String, String> {
    let level = arg_str(cmd, "level").ok_or("set_volume: missing 'level' argument")?;

    #[cfg(target_os = "macos")]
    {
        let script = match level.as_str() {
            "higher" | "increase" => "tell application \"System Events\" to key code 124", // volume up
            "lower" | "decrease" => "tell application \"System Events\" to key code 125", // volume down
            "mute" => "tell application \"System Events\" to key code 49",                // mute
            "unmute" => "tell application \"System Events\" to key code 49",              // unmute
            "max" | "100" => "tell application \"System Events\" to key code 124",
            "min" | "0" => "tell application \"System Events\" to key code 125",
            _ => {
                let percent: u32 = level.parse().unwrap_or(50);
                if percent > 50 {
                    "tell application \"System Events\" to key code 124"
                } else {
                    "tell application \"System Events\" to key code 125"
                }
            }
        };
        let status = std::process::Command::new("osascript")
            .args(["-e", script])
            .status()
            .map_err(|e| format!("osascript volume: {e}"))?;
        if status.success() {
            Ok(format!("volume set to {level}"))
        } else {
            Err(format!("volume set to {level} failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = level;
        Err("set_volume not supported on this platform".into())
    }
}

fn execute_mute_speaker(cmd: &CommandAction) -> Result<String, String> {
    let muted = arg_bool(cmd, "muted").ok_or("mute_speaker: missing 'muted' argument")?;

    #[cfg(target_os = "macos")]
    {
        // Press the mute key (F10 / key code 49).
        let status = std::process::Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to key code 49"])
            .status()
            .map_err(|e| format!("osascript mute: {e}"))?;
        if status.success() {
            Ok(if muted {
                "speaker muted"
            } else {
                "speaker unmuted"
            }
            .into())
        } else {
            Err(format!("mute_speaker failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = muted;
        Err("mute_speaker not supported on this platform".into())
    }
}

fn execute_keyboard_shortcut(cmd: &CommandAction) -> Result<String, String> {
    let action = arg_str(cmd, "action").ok_or("keyboard_shortcut: missing 'action' argument")?;

    #[cfg(target_os = "macos")]
    {
        let script = match action.as_str() {
            "select_all" => "tell application \"System Events\" to keystroke \"a\" using command down",
            "copy" => "tell application \"System Events\" to keystroke \"c\" using command down",
            "paste" => "tell application \"System Events\" to keystroke \"v\" using command down",
            "cut" => "tell application \"System Events\" to keystroke \"x\" using command down",
            "undo" => "tell application \"System Events\" to keystroke \"z\" using command down",
            "redo" => "tell application \"System Events\" to keystroke \"z\" using {command down, shift down}",
            "save" => "tell application \"System Events\" to keystroke \"s\" using command down",
            "save_as" => "tell application \"System Events\" to keystroke \"s\" using {command down, shift down}",
            "close_window" => "tell application \"System Events\" to keystroke \"w\" using command down",
            "minimize_window" => "tell application \"System Events\" to keystroke \"m\" using command down",
            "maximize_window" => "tell application \"System Events\" to keystroke \"m\" using {command down, control down}",
            "take_screenshot" => "screencapture -x /tmp/teletype_screenshot.png",
            "lock_screen" => "pmset displaysleepnow",
            "sleep" => "pmset sleepnow",
            "show_desktop" => "tell application \"Finder\" to set the view of folder \"Desktop\" to icon view",
            "refresh" => "tell application \"System Events\" to keystroke \"r\" using command down",
            "new_tab" => "tell application \"System Events\" to keystroke \"t\" using command down",
            "close_tab" => "tell application \"System Events\" to keystroke \"w\" using command down",
            "next_tab" => "tell application \"System Events\" to keystroke \"}\" using command down",
            "previous_tab" => "tell application \"System Events\" to keystroke \"{\" using command down",
            "go_back" => "tell application \"System Events\" to keystroke \"[\" using command down",
            "go_forward" => "tell application \"System Events\" to keystroke \"]\" using command down",
            other => return Err(format!("unknown keyboard action: {other}")),
        };

        let status = std::process::Command::new("osascript")
            .args(["-e", script])
            .status()
            .map_err(|e| format!("osascript {action}: {e}"))?;
        if status.success() {
            Ok(format!("executed {action}"))
        } else {
            Err(format!("keyboard_shortcut {action} failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = action;
        Err("keyboard_shortcut not supported on this platform".into())
    }
}

fn execute_open_settings(cmd: &CommandAction) -> Result<String, String> {
    let pane = arg_str(cmd, "pane");

    #[cfg(target_os = "macos")]
    {
        if let Some(ref pane) = pane {
            let url = format!("x-apple.systempreferences:{pane}");
            let status = std::process::Command::new("open")
                .arg(&url)
                .status()
                .map_err(|e| format!("open settings {pane}: {e}"))?;
            if status.success() {
                Ok(format!("opened settings: {pane}"))
            } else {
                Err(format!("open settings {pane} failed: {status}"))
            }
        } else {
            let status = std::process::Command::new("open")
                .args(["-a", "System Settings"])
                .status()
                .map_err(|e| format!("open System Settings: {e}"))?;
            if status.success() {
                Ok("opened System Settings".into())
            } else {
                Err(format!("open System Settings failed: {status}"))
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
        Err("open_settings not supported on this platform".into())
    }
}

fn execute_play_media(cmd: &CommandAction) -> Result<String, String> {
    let action = arg_str(cmd, "action").ok_or("play_media: missing 'action' argument")?;

    #[cfg(target_os = "macos")]
    {
        let key = match action.as_str() {
            "play" | "pause" => 49, // space
            "next" => 124,          // volume up (media next)
            "previous" => 125,      // volume down (media previous)
            _ => return Err(format!("unknown media action: {action}")),
        };
        let status = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!("tell application \"System Events\" to key code {key}"),
            ])
            .status()
            .map_err(|e| format!("osascript media: {e}"))?;
        if status.success() {
            Ok(format!("media: {action}"))
        } else {
            Err(format!("play_media {action} failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = action;
        Err("play_media not supported on this platform".into())
    }
}

fn execute_set_timer(cmd: &CommandAction) -> Result<String, String> {
    let duration = arg_str(cmd, "duration").ok_or("set_timer: missing 'duration' argument")?;
    // Open the Clock app with a timer.
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .args(["-a", "Clock"])
            .status()
            .map_err(|e| format!("open Clock: {e}"))?;
        if status.success() {
            Ok(format!("opened Clock (timer: {duration})"))
        } else {
            Err(format!("open Clock failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = duration;
        Err("set_timer not supported on this platform".into())
    }
}

// ---------------------------------------------------------------------------
// send_message with evidence
// ---------------------------------------------------------------------------

/// Runs `osascript -e <script>` and returns `Ok(stdout)` on success,
/// `Err` on a non-zero exit or spawn failure. `stderr` is appended to the
/// error on non-zero exits so a test can see why the script failed.
///
/// Bounded wait. A command-mode tool runs inline on the dictation worker, and
/// `osascript` blocks indefinitely behind a TCC consent sheet or a modal
/// dialog — the user sees a frozen app with no way to recover. After
/// [`OSASCRIPT_TIMEOUT`] the child is killed and the call reports a timeout.
#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> std::result::Result<String, String> {
    use std::io::Read as _;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    const OSASCRIPT_TIMEOUT: Duration = Duration::from_secs(8);
    const POLL: Duration = Duration::from_millis(50);

    let mut child = std::process::Command::new("osascript")
        .args(["-e", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("osascript: {e}"))?;

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < OSASCRIPT_TIMEOUT => {
                std::thread::sleep(POLL);
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "osascript did not finish within {}s (a permission prompt or modal dialog may be blocking it)",
                    OSASCRIPT_TIMEOUT.as_secs()
                ));
            }
            Err(e) => return Err(format!("osascript: {e}")),
        }
    };

    // The child has exited, so its pipes are closed and these reads cannot
    // block. Both streams stay well under the pipe buffer for our scripts.
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }

    if !status.success() {
        return Err(format!("osascript exited with {status}: {}", stderr.trim()));
    }
    Ok(stdout)
}

/// Maps a raw AppleScript failure to a short, user-visible reason.
///
/// A dictionary without a message-history class fails at compile time with
/// -2741 (syntax error) or -1728 (unknown property). That is "history is not
/// scriptable here", not "the sent text did not match".
#[cfg(target_os = "macos")]
fn evidence_failure_reason(err: &str) -> String {
    if err.contains("(-2741)") || err.contains("(-1728)") || err.contains("syntax error") {
        "Messages does not expose message history on this macOS".to_string()
    } else {
        err.to_string()
    }
}

fn execute_send_message(cmd: &CommandAction) -> Result<String, String> {
    let to = arg_str(cmd, "to").ok_or("send_message: missing 'to' argument")?;
    let text = arg_str(cmd, "text").ok_or("send_message: missing 'text' argument")?;

    #[cfg(target_os = "macos")]
    {
        // Messages only understands handles. A spoken name must become a
        // real handle through Contacts before it reaches the send script —
        // otherwise Messages fabricates a participant with that literal name.
        let handle = if looks_like_handle(&to) {
            to.clone()
        } else {
            resolve_contact_handle(&to)?
        };
        // Show the resolved handle when a name was spoken: without it the
        // user cannot tell which of a contact's addresses was used.
        let label = if handle == to {
            to.clone()
        } else {
            format!("{to} via {handle}")
        };

        // Step 1: send. A non-zero exit means the send itself failed —
        // that's the only `Err` path for this tool.
        let send_script = messages_script(&handle, &text);
        if let Err(e) = run_osascript(&send_script) {
            let reason = format!("send_message to {to} failed ({e})");
            tracing::warn!("[commands] send_message evidence={reason}");
            return Err(reason);
        }

        // Step 2: evidence. Retry up to 2 reads, ~150 ms apart, total
        // sleep ≤ ~300 ms. If the first read succeeds and matches, we're
        // done; otherwise one retry, then give up and report unverified.
        let evidence_script = messages_evidence_script(&handle);
        let mut last_err: Option<String> = None;
        for attempt in 0..2 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            match run_osascript(&evidence_script) {
                Ok(raw) => {
                    let evidence = parse_evidence_output(&raw, &text);
                    // A mismatch can be a lagging write, so give the last
                    // read a second chance before calling it unverified.
                    if matches!(&evidence, SendEvidence::Verified) || attempt == 1 {
                        if let SendEvidence::Unverified(reason) = &evidence {
                            tracing::warn!(
                                "[commands] send_message to {to}: unverified ({reason})"
                            );
                        }
                        return format_send_evidence(&label, &evidence);
                    }
                    if let SendEvidence::Unverified(reason) = evidence {
                        last_err = Some(reason);
                    }
                }
                Err(e) => {
                    // Script failed (e.g. no conversation with this
                    // recipient, or the macOS version's Messages AppleScript
                    // API doesn't expose history). Not fatal — just means
                    // we can't verify.
                    last_err = Some(evidence_failure_reason(&e));
                }
            }
        }
        let reason = last_err
            .map(|e| format!("evidence read failed: {e}"))
            .unwrap_or_else(|| "evidence read unavailable".into());
        tracing::warn!("[commands] send_message to {to}: unverified ({reason})");
        format_send_evidence(&label, &SendEvidence::Unverified(reason))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (to, text);
        Err("send_message not supported on this platform".into())
    }
}

// ---------------------------------------------------------------------------
// W1 new tools: compose_email, create_calendar_event, open_file, type_text
// ---------------------------------------------------------------------------

/// Builds the Gmail compose URL for a `compose_email` call.
///
/// Pure. Returns `Err` on validation failure so the handler can map it to
/// the tool's error string. URL-encodes `to`, `subject`, `body` (and `cc`
/// when present) so `&`, spaces, and unicode are safe in the query string.
fn build_gmail_compose_url(
    to: &str,
    subject: &str,
    body: &str,
    cc: Option<&str>,
) -> std::result::Result<String, String> {
    if to.is_empty() {
        return Err("compose_email: 'to' is empty".into());
    }
    if cc.is_some_and(str::is_empty) {
        return Err("compose_email: 'cc' is empty".into());
    }
    let enc_to = url::form_urlencoded::byte_serialize(to.as_bytes()).collect::<String>();
    let enc_su = url::form_urlencoded::byte_serialize(subject.as_bytes()).collect::<String>();
    let enc_body = url::form_urlencoded::byte_serialize(body.as_bytes()).collect::<String>();
    let mut url = format!(
        "https://mail.google.com/mail/?view=cm&fs=1&to={enc_to}&su={enc_su}&body={enc_body}"
    );
    if let Some(cc) = cc {
        let enc_cc = url::form_urlencoded::byte_serialize(cc.as_bytes()).collect::<String>();
        url.push_str(&format!("&cc={enc_cc}"));
    }
    Ok(url)
}

/// Validates an email address (or address list) for the `compose_email`
/// tool: non-empty, each comma-separated entry has exactly one `@`, no
/// whitespace or control characters.
fn validate_email_addr(label: &str, value: &str) -> std::result::Result<(), String> {
    if value.is_empty() {
        return Err(format!("compose_email: '{label}' is empty"));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!(
            "compose_email: '{label}' contains whitespace or control characters"
        ));
    }
    for part in value.split(',') {
        if part.matches('@').count() != 1 {
            return Err(format!(
                "compose_email: '{label}' entry '{part}' does not contain exactly one '@'"
            ));
        }
    }
    Ok(())
}

fn execute_compose_email(cmd: &CommandAction) -> Result<String, String> {
    let to = arg_str(cmd, "to").ok_or("compose_email: missing 'to' argument")?;
    let subject = arg_str(cmd, "subject").ok_or("compose_email: missing 'subject' argument")?;
    let body = arg_str(cmd, "body").ok_or("compose_email: missing 'body' argument")?;
    let cc = arg_str(cmd, "cc");

    validate_email_addr("to", &to)?;
    if let Some(ref cc) = cc {
        validate_email_addr("cc", cc)?;
    }
    let url = build_gmail_compose_url(&to, &subject, &body, cc.as_deref())?;

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg(&url)
            .status()
            .map_err(|e| format!("open gmail draft: {e}"))?;
        if status.success() {
            Ok(format!("opened a Gmail draft to {to} (not sent)"))
        } else {
            Err(format!("open gmail draft failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err("compose_email not supported on this platform".into())
    }
}

/// Parses a `YYYY-MM-DDTHH:MM` local-time string. Returns
/// `Some((year, month, day, hour, minute))` or `Err` with a human reason.
///
/// Pure. No timezone math — the W1 contract is explicit that the string is
/// *local* 24h and Google Calendar interprets it in the user's calendar
/// timezone when the URL has no `Z`.
fn parse_local_datetime(s: &str) -> std::result::Result<(i32, u32, u32, u32, u32), String> {
    // Expected: exactly 16 chars: YYYY-MM-DDTHH:MM
    if s.len() != 16 {
        return Err(format!("expected YYYY-MM-DDTHH:MM, got {s:?}"));
    }
    // Byte indexing below is only safe on ASCII; a 16-byte string with a
    // multibyte character can split a char boundary at `get(0..4)`.
    if !s.is_ascii() {
        return Err(format!("expected ASCII YYYY-MM-DDTHH:MM, got {s:?}"));
    }
    // The year must be four ASCII digits. `parse::<i32>()` happily accepts
    // "+123" and "-123", which then reach the encoded URL as `+1230215…`
    // and are rejected by Google with an opaque error.
    if !s.as_bytes()[0..4].iter().all(u8::is_ascii_digit) {
        return Err(format!("year is not four digits: {:?}", &s[0..4]));
    }
    let year: i32 = s
        .get(0..4)
        .unwrap()
        .parse()
        .map_err(|_| "year not a number")?;
    if !(1900..=9999).contains(&year) {
        return Err(format!("year out of range 1900-9999: {year}"));
    }
    if s.as_bytes()[4] != b'-' {
        return Err(format!(
            "expected '-' at position 5, got {:?}",
            s.as_bytes()[4] as char
        ));
    }
    let month: u32 = s
        .get(5..7)
        .unwrap()
        .parse()
        .map_err(|_| "month not a number")?;
    if s.as_bytes()[7] != b'-' {
        return Err(format!(
            "expected '-' at position 8, got {:?}",
            s.as_bytes()[7] as char
        ));
    }
    let day: u32 = s
        .get(8..10)
        .unwrap()
        .parse()
        .map_err(|_| "day not a number")?;
    if s.as_bytes()[10] != b'T' {
        return Err(format!(
            "expected 'T' at position 11, got {:?}",
            s.as_bytes()[10] as char
        ));
    }
    let hour: u32 = s
        .get(11..13)
        .unwrap()
        .parse()
        .map_err(|_| "hour not a number")?;
    if s.as_bytes()[13] != b':' {
        return Err(format!(
            "expected ':' at position 14, got {:?}",
            s.as_bytes()[13] as char
        ));
    }
    let minute: u32 = s
        .get(14..16)
        .unwrap()
        .parse()
        .map_err(|_| "minute not a number")?;

    if !(1..=12).contains(&month) {
        return Err(format!("month out of range 1-12: {month}"));
    }
    if !(1..=31).contains(&day) {
        return Err(format!("day out of range 1-31: {day}"));
    }
    if !(0..=23).contains(&hour) {
        return Err(format!("hour out of range 0-23: {hour}"));
    }
    if !(0..=59).contains(&minute) {
        return Err(format!("minute out of range 0-59: {minute}"));
    }
    Ok((year, month, day, hour, minute))
}

/// Encodes a parsed local datetime into Google Calendar's `dates=` format:
/// `YYYYMMDDTHHMMSS` with a trailing `00` seconds and *no* `Z`, so Google
/// uses the calendar's timezone.
fn encode_gcal_datetime(y: i32, m: u32, d: u32, h: u32, min: u32) -> String {
    format!("{y:04}{m:02}{d:02}T{h:02}{min:02}00")
}

/// Builds the Google Calendar template URL for a `create_calendar_event`
/// call. Pure. Validates both datetimes and requires `end` to be
/// lexicographically after `start` (in the encoded form, which is the
/// same as chronological order for `YYYYMMDDTHHMMSS`).
fn build_gcal_template_url(
    title: &str,
    start: &str,
    end: &str,
    location: Option<&str>,
    details: Option<&str>,
) -> std::result::Result<String, String> {
    if title.is_empty() {
        return Err("create_calendar_event: 'title' is empty".into());
    }
    let s =
        parse_local_datetime(start).map_err(|e| format!("create_calendar_event: start: {e}"))?;
    let e = parse_local_datetime(end).map_err(|e| format!("create_calendar_event: end: {e}"))?;
    let s_enc = encode_gcal_datetime(s.0, s.1, s.2, s.3, s.4);
    let e_enc = encode_gcal_datetime(e.0, e.1, e.2, e.3, e.4);
    if s_enc >= e_enc {
        return Err(format!(
            "create_calendar_event: end ({end}) must be after start ({start})"
        ));
    }
    let enc_title = url::form_urlencoded::byte_serialize(title.as_bytes()).collect::<String>();
    let mut url = format!(
        "https://calendar.google.com/calendar/render?action=TEMPLATE&text={enc_title}&dates={s_enc}/{e_enc}"
    );
    if let Some(loc) = location.filter(|l| !l.is_empty()) {
        let enc_loc = url::form_urlencoded::byte_serialize(loc.as_bytes()).collect::<String>();
        url.push_str(&format!("&location={enc_loc}"));
    }
    if let Some(det) = details.filter(|d| !d.is_empty()) {
        let enc_det = url::form_urlencoded::byte_serialize(det.as_bytes()).collect::<String>();
        url.push_str(&format!("&details={enc_det}"));
    }
    Ok(url)
}

fn execute_create_calendar_event(cmd: &CommandAction) -> Result<String, String> {
    let title = arg_str(cmd, "title").ok_or("create_calendar_event: missing 'title' argument")?;
    let start = arg_str(cmd, "start").ok_or("create_calendar_event: missing 'start' argument")?;
    let end = arg_str(cmd, "end").ok_or("create_calendar_event: missing 'end' argument")?;
    let location = arg_str(cmd, "location");
    let details = arg_str(cmd, "details");

    let url = build_gcal_template_url(
        &title,
        &start,
        &end,
        location.as_deref(),
        details.as_deref(),
    )?;

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg(&url)
            .status()
            .map_err(|e| format!("open gcal draft: {e}"))?;
        if status.success() {
            Ok(format!("opened a Google Calendar draft: {title}"))
        } else {
            Err(format!("open gcal draft failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err("create_calendar_event not supported on this platform".into())
    }
}

/// Resolves an `open_file` `path` argument to an absolute path that must
/// exist.
///
/// Pure (no fs I/O — existence is checked by the handler so the resolver
/// is unit-testable without touching disk). Accepts:
/// - `~` or `~/...` → expanded to `$HOME/...`
/// - `downloads`, `documents`, `desktop`, `home` (case-insensitive,
///   bare word) → `$HOME/Downloads`, `$HOME/Documents`, `$HOME/Desktop`,
///   `$HOME`
/// - any other absolute path → returned as-is
///
/// Returns `Err` for a relative path that is not one of the bare aliases,
/// or for a path that is empty after expansion.
fn resolve_open_file_path(path: &str) -> std::result::Result<std::path::PathBuf, String> {
    if path.is_empty() {
        return Err("open_file: 'path' is empty".into());
    }
    let trimmed = path.trim();
    let lower = trimmed.to_lowercase();

    // Bare aliases.
    match lower.as_str() {
        "downloads" => {
            let home = std::env::var("HOME").map_err(|_| "open_file: $HOME unset")?;
            return Ok(std::path::PathBuf::from(home).join("Downloads"));
        }
        "documents" => {
            let home = std::env::var("HOME").map_err(|_| "open_file: $HOME unset")?;
            return Ok(std::path::PathBuf::from(home).join("Documents"));
        }
        "desktop" => {
            let home = std::env::var("HOME").map_err(|_| "open_file: $HOME unset")?;
            return Ok(std::path::PathBuf::from(home).join("Desktop"));
        }
        "home" => {
            let home = std::env::var("HOME").map_err(|_| "open_file: $HOME unset")?;
            return Ok(std::path::PathBuf::from(home));
        }
        _ => {}
    }

    // `~` or `~/...`.
    if trimmed == "~" {
        let home = std::env::var("HOME").map_err(|_| "open_file: $HOME unset")?;
        return Ok(std::path::PathBuf::from(home));
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = std::env::var("HOME").map_err(|_| "open_file: $HOME unset")?;
        let joined = std::path::Path::new(&home).join(rest);
        if !joined.is_absolute() {
            return Err(format!(
                "open_file: '{path}' does not expand to an absolute path"
            ));
        }
        return Ok(joined);
    }

    // Absolute path.
    let pb = std::path::PathBuf::from(trimmed);
    if !pb.is_absolute() {
        return Err(format!(
            "open_file: '{path}' is not an absolute path (and not one of: ~, downloads, documents, desktop, home)"
        ));
    }
    Ok(pb)
}

fn execute_open_file(cmd: &CommandAction) -> Result<String, String> {
    let path = arg_str(cmd, "path").ok_or("open_file: missing 'path' argument")?;
    let resolved = resolve_open_file_path(&path)?;

    // Existence check (fs I/O, so it lives in the handler, not the pure
    // resolver).
    if let Err(e) = std::fs::metadata(&resolved) {
        return Err(format!(
            "open_file: path does not exist: {} ({e})",
            resolved.display()
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg(&resolved)
            .status()
            .map_err(|e| format!("open file: {e}"))?;
        if status.success() {
            Ok(format!("opened {}", resolved.display()))
        } else {
            Err(format!("open file failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = resolved;
        Err("open_file not supported on this platform".into())
    }
}

// ---------------------------------------------------------------------------
// type_text
// ---------------------------------------------------------------------------

/// Maximum characters for a single `type_text` payload. Larger payloads
/// are rejected before any clipboard/keystroke work — a single paste is
/// the whole point of the tool, and a 50 KB paste into the wrong window
/// is a much worse failure than a clean `Err`.
pub const TYPE_TEXT_MAX_CHARS: usize = 5000;

/// The identity of the window a `type_text` call is about to paste into.
///
/// Compared before and after the clipboard set / keystroke to catch the
/// case where the user (or another app) switched focus between the
/// initial check and the paste. When both sides have a `title`, the
/// titles must match; when both are `None`, app-only identity is allowed
/// (the result string says so).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowIdentity {
    pub app_id: String,
    pub title: Option<String>,
}

/// Pure comparison of two [`WindowIdentity`] values.
///
/// - app ids must match when both are non-empty;
/// - titles must match when both are `Some`;
/// - when both titles are `None`, app-only identity is allowed (returns
///   `true` when the app ids agree, or when both app ids are empty —
///   the caller is responsible for the "identity unavailable" variant).
pub fn same_window(a: &WindowIdentity, b: &WindowIdentity) -> bool {
    // App ids: must match when both are non-empty.
    if !a.app_id.is_empty() && !b.app_id.is_empty() && a.app_id != b.app_id {
        return false;
    }
    // Titles: must match when both are Some.
    match (&a.title, &b.title) {
        (Some(ta), Some(tb)) => ta == tb,
        _ => true,
    }
}

/// Captures the current frontmost window identity for a `type_text`
/// call. The app id comes from the already-detected `ctx` (the dictation
/// pipeline captured it moments earlier); the title is read live from the
/// AX tree at this moment.
#[cfg(target_os = "macos")]
fn capture_window_identity(ctx: &ApplicationContext) -> WindowIdentity {
    // Read the live frontmost app at capture time. Reusing
    // `ctx.application_id` for both captures would make an app switch
    // invisible to `same_window`, leaving the title as the only signal.
    let live_app_id = crate::platform::create()
        .active_application()
        .map(|app| app.application_id)
        .filter(|id| !id.is_empty());
    WindowIdentity {
        app_id: live_app_id.unwrap_or_else(|| ctx.application_id.clone()),
        title: crate::ax_text::focused_window_title(),
    }
}

fn execute_type_text(cmd: &CommandAction, ctx: &ApplicationContext) -> Result<String, String> {
    let text = arg_str(cmd, "text").ok_or("type_text: missing 'text' argument")?;
    if text.is_empty() {
        return Err("type_text: 'text' is empty".into());
    }
    if text.chars().count() > TYPE_TEXT_MAX_CHARS {
        return Err(format!(
            "type_text: 'text' is {} characters, max is {TYPE_TEXT_MAX_CHARS}",
            text.chars().count()
        ));
    }

    #[cfg(target_os = "macos")]
    {
        // Refuse to type into a secure field (passwords, OTP boxes). The
        // dictation pipeline already reads this for context, so it is a
        // known-good read.
        if focused_field_is_secure() {
            return Err("type_text: the focused field is secure; refusing to type".into());
        }

        // Pre-paste identity capture. Both the app id and the title are
        // read live (ctx is only the fallback when the platform read
        // fails). A mismatch between the pre-paste and the
        // immediately-pre-keystroke captures is the signal that the user
        // switched windows.
        let pre = capture_window_identity(ctx);

        // The take began in `ctx.application_id`; verify the frontmost app is
        // still that app before touching the clipboard or pressing a key.
        if !ctx.application_id.is_empty()
            && !pre.app_id.is_empty()
            && pre.app_id != ctx.application_id
        {
            return Err("target app changed; nothing was typed".into());
        }

        use teletype_core::injector::ClipboardGuard as _;

        // Snapshot the *whole* pasteboard, not just its text. `arboard` can
        // only read and write text and images, and either setter replaces the
        // entire pasteboard, so the old text-only snapshot destroyed a file
        // copied in Finder or rich text copied from a browser on every
        // type_text. `MacClipboardGuard` copies every item and representation
        // (UTI) and writes the same structure back.
        let clipboard_guard = crate::clipboard::MacClipboardGuard::new();
        let clipboard_snapshot = clipboard_guard.snapshot();
        // Text-only fallback for the rare case the full snapshot fails: at
        // least put the user's text back.
        let prev_text = if clipboard_snapshot.is_none() {
            let _pasteboard = teletype_core::injector::pasteboard_lock();
            arboard::Clipboard::new()
                .ok()
                .and_then(|mut c| c.get_text().ok())
        } else {
            None
        };
        let restore_clipboard = || match &clipboard_snapshot {
            // No `extra`: this text is a command's payload, not a dictation,
            // so the user's clipboard goes back exactly as it was rather than
            // growing an item.
            Some(snapshot) => clipboard_guard.restore(snapshot, None),
            None => restore_previous_text(prev_text.as_deref()),
        };

        // Set the clipboard to the text we want to paste.
        {
            let _pasteboard = teletype_core::injector::pasteboard_lock();
            let mut clip = arboard::Clipboard::new()
                .map_err(|e| format!("type_text: clipboard unavailable: {e}"))?;
            clip.set_text(text.clone())
                .map_err(|e| format!("type_text: clipboard set failed: {e}"))?;
        }

        // Small settle so the pasteboard is fully written before the
        // keystroke (matches the delivery_io pacing).
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Re-read identity *immediately* before the paste. This is the
        // critical re-check: the window could have changed in the ~100 ms
        // since the pre-check.
        let post = capture_window_identity(ctx);
        if !same_window(&pre, &post) {
            // Restore the clipboard best-effort before returning.
            restore_clipboard();
            return Err("target window changed; nothing was typed".into());
        }

        // Re-check the secure-field flag immediately before the keystroke:
        // focus can move during the settle, and the first check ran before
        // the clipboard was touched. Pasting into a password box is exactly
        // what this guard exists to prevent.
        if focused_field_is_secure() {
            restore_clipboard();
            return Err("type_text: the focused field became secure; refusing to type".into());
        }

        // Fire ⌘V via the module's own enigo. Inlined (mirroring
        // `delivery_io::macos::paste_key`) because that helper lives in a
        // private submodule and is not re-exported at the crate root; the
        // keystroke is identical (⌘ + ANSI 'V' keycode 9). Never presses Enter.
        {
            let mut enigo_guard = paste_enigo()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if enigo_guard.is_none() {
                match enigo::Enigo::new(&enigo::Settings::default()) {
                    Ok(e) => *enigo_guard = Some(e),
                    Err(e) => {
                        restore_clipboard();
                        return Err(format!("type_text: enigo unavailable: {e}"));
                    }
                }
            }
            let enigo = enigo_guard.as_mut().unwrap();
            use enigo::Keyboard as _;
            let result = (|| {
                enigo.key(enigo::Key::Meta, enigo::Direction::Press)?;
                let r = enigo.key(enigo::Key::Other(9), enigo::Direction::Click);
                enigo.key(enigo::Key::Meta, enigo::Direction::Release)?;
                r
            })();
            if let Err(e) = result {
                restore_clipboard();
                return Err(format!("type_text: paste keystroke failed: {e}"));
            }
        }

        // Settle for the paste to land.
        std::thread::sleep(std::time::Duration::from_millis(350));

        // Restore the user's clipboard, best-effort.
        restore_clipboard();

        // Result string: say what was verified. When the pre/post titles
        // were both `None`, the comparison was app-only — say so.
        let app_name = if ctx.application_name.is_empty() {
            ctx.application_id.clone()
        } else {
            ctx.application_name.clone()
        };
        let verified = match (&pre.title, &post.title) {
            (Some(_), Some(_)) => "window verified".to_string(),
            (None, None) => "verified (app only, no window title)".to_string(),
            // One side lost its title between the two captures. `same_window`
            // allows it, but the title was only compared once, so this is not
            // a full window verification.
            _ => "verified (app only, window title read once)".to_string(),
        };
        Ok(format!(
            "pasted {} characters into {app_name} ({verified})",
            text.chars().count()
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (ctx, text);
        Err("type_text not supported on this platform".into())
    }
}

/// Best-effort restore of a previous clipboard *text*, used only when the
/// full pasteboard snapshot is unavailable. Logs (does not error) on
/// failure — the type itself already happened, so an un-restored clipboard
/// is a much smaller problem than a missed paste.
#[cfg(target_os = "macos")]
fn restore_previous_text(prev: Option<&str>) {
    let Some(prev) = prev else {
        return;
    };
    let _pasteboard = teletype_core::injector::pasteboard_lock();
    match arboard::Clipboard::new() {
        Ok(mut c) => {
            if let Err(e) = c.set_text(prev.to_string()) {
                tracing::warn!("[commands] type_text: clipboard restore failed: {e}");
            }
        }
        Err(e) => {
            tracing::warn!("[commands] type_text: clipboard restore failed: {e}");
        }
    }
}

/// `true` when the focused field declares itself secure (password, OTP).
///
/// Fails *open* when the accessibility tree gives no answer: most native apps
/// expose `AXSecureTextField`, but a web view can refuse the whole query, and
/// failing closed would make `type_text` unusable there.
#[cfg(target_os = "macos")]
fn focused_field_is_secure() -> bool {
    crate::ax_text::read_focused_context(0, 0).is_some_and(|f| f.secure)
}

/// Process-lifetime shared enigo for the `type_text` paste. The dictation
/// pipeline has its own enigo; a second one here keeps this module
/// self-contained and avoids reaching into the pipeline's internals.
#[cfg(target_os = "macos")]
fn paste_enigo() -> &'static Mutex<Option<enigo::Enigo>> {
    static ENIGO: OnceLock<Mutex<Option<enigo::Enigo>>> = OnceLock::new();
    ENIGO.get_or_init(|| Mutex::new(None))
}

// ---------------------------------------------------------------------------
// Tests (all offline; no osascript, no GUI, no clipboard, no keystrokes)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cmd(tool: &str, args: serde_json::Value) -> CommandAction {
        CommandAction {
            tool: tool.into(),
            arguments: args,
        }
    }

    fn ctx_default() -> ApplicationContext {
        ApplicationContext::default()
    }

    // ------------------------------------------------------------------
    // Existing tests (unchanged from the pre-W1 file)
    // ------------------------------------------------------------------

    #[test]
    fn resolve_app_name_vscode() {
        assert_eq!(resolve_app_name("vs code"), "Visual Studio Code");
        assert_eq!(resolve_app_name("VS Code"), "Visual Studio Code");
        assert_eq!(resolve_app_name("vscode"), "Visual Studio Code");
    }

    #[test]
    fn resolve_app_name_terminal() {
        assert_eq!(resolve_app_name("terminal"), "Terminal");
        assert_eq!(resolve_app_name("command line"), "Terminal");
    }

    #[test]
    fn resolve_app_name_capitalizes() {
        // "safari" should match the installed Safari app.
        assert_eq!(resolve_app_name("safari"), "Safari");
        // "brave" may match "Brave Browser" if installed, or capitalize to "Brave".
        let result = resolve_app_name("brave");
        assert!(
            result == "Brave" || result == "Brave Browser",
            "unexpected result: {result}"
        );
    }

    #[test]
    fn unknown_tool_returns_error() {
        let c = cmd("unknown_tool", json!({}));
        let ctx = ctx_default();
        assert!(execute(&c, &ctx).is_err());
    }

    #[test]
    fn unknown_tool_still_rejected_after_w1() {
        // W1 added four tools; an unknown tool must still be rejected the
        // same way (exact error text unchanged from pre-W1).
        let c = cmd("open_spreadsheet", json!({}));
        let ctx = ctx_default();
        let err = execute(&c, &ctx).unwrap_err();
        assert_eq!(err, "unknown command tool: open_spreadsheet");
    }

    #[test]
    fn open_app_missing_arg_returns_error() {
        let c = cmd("open_app", json!({}));
        let ctx = ctx_default();
        assert!(execute(&c, &ctx).is_err());
    }

    #[test]
    fn control_characters_are_rejected_before_any_tool_runs() {
        // The P2.7 fallback parses LLM replies with exactly this function,
        // so this is the fallback path's output hitting the executor: a
        // hallucinated (or crafted) newline must never reach a tool. The
        // rejection happens before dispatch — `send_message` below would
        // otherwise run osascript.
        let c = teletype_inference::parse_command_json(
            "{\"name\":\"send_message\",\"arguments\":{\"to\":\"Sara\",\"text\":\"hi\\nend tell\\ndo shell script \\\"rm -rf ~\\\"\"}}",
        )
        .expect("fallback parser accepts the shape");
        let ctx = ctx_default();
        let err = execute(&c, &ctx).unwrap_err();
        assert!(
            err.contains("control characters"),
            "unexpected error: {err}"
        );
        assert!(err.contains("'text'"), "should name the field: {err}");
    }

    #[test]
    fn control_characters_are_rejected_in_every_string_argument() {
        for reply in [
            "{\"name\":\"open_app\",\"arguments\":{\"app\":\"Safari\\n; open evil.com\"}}",
            "{\"name\":\"open_url\",\"arguments\":{\"url\":\"evil.com\\u0007\"}}",
            "{\"name\":\"set_timer\",\"arguments\":{\"duration\":\"5 minutes\\u0000\"}}",
        ] {
            let c = teletype_inference::parse_command_json(reply).expect("parsed");
            let ctx = ctx_default();
            let err = execute(&c, &ctx).unwrap_err();
            assert!(
                err.contains("control characters"),
                "{reply} should be rejected, got: {err}"
            );
        }
    }

    #[test]
    fn apple_script_escaping_neutralizes_quotes_and_backslashes() {
        // A backslash-quote sequence used to end the string literal early
        // (`\"` became `\\"` — escaped backslash + raw quote), the classic
        // AppleScript injection. Both characters must be escaped, backslash
        // first.
        let script = messages_script("Sara", "say \"hi\" \\ and ; rm -rf");
        assert!(
            script.contains("send \"say \\\"hi\\\" \\\\ and ; rm -rf\" to participant \"Sara\""),
            "escaping failed: {script}"
        );
        // The recipient is a string literal too — a quote in a contact name
        // cannot break out.
        let script = messages_script("Sa\"ra", "hello");
        assert!(
            script.contains("participant \"Sa\\\"ra\""),
            "recipient escaping failed: {script}"
        );
    }

    #[test]
    fn ordinary_send_message_values_pass_the_guard() {
        // Apostrophes and unicode are common in real messages and must not
        // be rejected — only control characters are.
        let c = teletype_inference::parse_command_json(
            "{\"name\":\"send_message\",\"arguments\":{\"to\":\"Sara O'Hara\",\"text\":\"I'll be there — 10:30!\"}}",
        )
        .expect("parsed");
        assert!(reject_control_characters(&c).is_ok());
    }

    #[test]
    fn looks_like_handle_accepts_addresses_and_numbers() {
        for h in [
            "a@b.com",
            "+15551234567",
            "15551234567",
            "+1 (555) 123-4567",
            "e:a@b.com",
            "p:+15551234567",
        ] {
            assert!(looks_like_handle(h), "{h} should be a handle");
        }
        for n in ["Sara", "Sara O'Hara", "", "Mom", "+", "p:"] {
            assert!(!looks_like_handle(n), "{n} should not be a handle");
        }
    }

    #[test]
    fn contacts_lookup_script_escapes_the_name() {
        let script = contacts_lookup_script("Sa\"ra");
        assert!(
            script.contains("whose name is \"Sa\\\"ra\""),
            "escaping failed: {script}"
        );
    }

    #[test]
    fn contacts_lookup_reports_not_found_and_no_handle() {
        assert_eq!(parse_contact_lookup(""), ContactLookup::NotFound);
        assert_eq!(parse_contact_lookup("\n \n"), ContactLookup::NotFound);
        assert_eq!(
            parse_contact_lookup("PERSON\nPERSON\n"),
            ContactLookup::NoHandle
        );
    }

    #[test]
    fn contacts_lookup_prefers_mobile_then_email_then_any_phone() {
        let raw = "PERSON\n\
                   EMAIL\t_$!<Work>!$_\tali@example.com\n\
                   PHONE\t_$!<Home>!$_\t+1 (555) 000-1111\n\
                   PHONE\t_$!<Mobile>!$_\t+1 (555) 222-3333\n";
        assert_eq!(
            parse_contact_lookup(raw),
            ContactLookup::Found("+15552223333".into())
        );

        let raw = "PERSON\n\
                   PHONE\t_$!<Home>!$_\t+1 (555) 000-1111\n\
                   EMAIL\t_$!<Work>!$_\tali@example.com\n";
        assert_eq!(
            parse_contact_lookup(raw),
            ContactLookup::Found("ali@example.com".into())
        );

        let raw = "PERSON\nPHONE\t_$!<Home>!$_\t+1 (555) 000-1111\n";
        assert_eq!(
            parse_contact_lookup(raw),
            ContactLookup::Found("+15550001111".into())
        );
    }

    #[test]
    fn contacts_lookup_ignores_cards_without_handles() {
        // The real case: two cards share the name, only one has an address.
        // That is a unique usable match, not an ambiguity.
        let raw = "PERSON\nEMAIL\t_$!<Work>!$_\tali@cafu.com\nPERSON\n";
        assert_eq!(
            parse_contact_lookup(raw),
            ContactLookup::Found("ali@cafu.com".into())
        );
    }

    #[test]
    fn contacts_lookup_reports_ambiguity_and_skips_unusable_values() {
        let raw = "PERSON\nEMAIL\t_$!<Work>!$_\ta@b.com\n\
                   PERSON\nPHONE\t_$!<Mobile>!$_\t+15550000000\n";
        assert_eq!(parse_contact_lookup(raw), ContactLookup::Ambiguous(2));

        let raw = "PERSON\nPHONE\t_$!<Mobile>!$_\tcall me\nEMAIL\t_$!<Work>!$_\t\n";
        assert_eq!(parse_contact_lookup(raw), ContactLookup::NoHandle);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn contacts_lookup_error_hints_only_on_denied_access() {
        assert!(contacts_lookup_error("osascript exited: (-1743)")
            .contains("Privacy & Security > Contacts"));
        assert!(!contacts_lookup_error("osascript exited: (-600)")
            .contains("Privacy & Security > Contacts"));
    }

    #[test]
    fn levenshtein_similarity_identical() {
        assert!((levenshtein_similarity("hello", "hello") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn levenshtein_similarity_completely_different() {
        assert!(levenshtein_similarity("abc", "xyz") < 0.5);
    }

    #[test]
    fn levenshtein_similarity_close_match() {
        // "terax" vs "terminal" should have a reasonable similarity.
        let sim = levenshtein_similarity("terax", "terminal");
        assert!(sim > 0.4, "expected similarity > 0.4, got {sim}");
    }

    #[test]
    fn levenshtein_similarity_empty_strings() {
        assert!((levenshtein_similarity("", "") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn levenshtein_similarity_one_empty() {
        assert!(levenshtein_similarity("", "hello") < 0.5);
    }

    // ------------------------------------------------------------------
    // W1: DryRunEffects
    // ------------------------------------------------------------------

    #[test]
    fn dry_run_records_without_executing() {
        // A dry-run of every original tool must record one "would …" line
        // each, return Ok, and never spawn a process (the recorded line
        // itself is the proof — the returned string equals the recorded
        // line, and no tool side effect is observable).
        let dry = DryRunEffects::new();
        let ctx = ctx_default();
        let cases = [
            ("open_app", json!({"app": "safari"})),
            ("open_url", json!({"url": "https://example.com"})),
            ("set_brightness", json!({"level": "higher"})),
            ("set_volume", json!({"level": "mute"})),
            ("mute_speaker", json!({"muted": true})),
            ("keyboard_shortcut", json!({"action": "copy"})),
            ("open_settings", json!({"pane": "keyboard"})),
            ("play_media", json!({"action": "pause"})),
            ("set_timer", json!({"duration": "5 minutes"})),
            ("send_message", json!({"to": "Sara", "text": "hi"})),
            (
                "compose_email",
                json!({"to": "a@b.com", "subject": "s", "body": "b"}),
            ),
            (
                "create_calendar_event",
                json!({"title": "t", "start": "2026-01-01T10:00", "end": "2026-01-01T11:00"}),
            ),
            ("open_file", json!({"path": "~/Documents"})),
            ("type_text", json!({"text": "hello"})),
        ];
        for (tool, args) in &cases {
            let c = cmd(tool, args.clone());
            let out = execute_with(&dry, &c, &ctx).expect("dry-run should be Ok");
            assert!(
                out.starts_with(&format!("would {tool}")),
                "expected 'would {tool} …', got {out}"
            );
        }
        // 14 lines recorded, one per dispatch.
        let recorded = dry.recorded();
        assert_eq!(recorded.len(), cases.len(), "one line per tool");
        for (i, (tool, _)) in cases.iter().enumerate() {
            assert!(recorded[i].starts_with(&format!("would {tool}")));
        }
    }

    #[test]
    fn dry_run_unknown_tool_is_rejected_not_recorded() {
        // Validation runs before effects: an unknown tool must produce an
        // error *and* must not be recorded.
        let dry = DryRunEffects::new();
        let ctx = ctx_default();
        let c = cmd("nope", json!({}));
        let err = execute_with(&dry, &c, &ctx).unwrap_err();
        assert!(err.starts_with("unknown command tool:"), "got: {err}");
        assert!(
            dry.recorded().is_empty(),
            "unknown tool must not be recorded"
        );
    }

    // ------------------------------------------------------------------
    // W1: send_message evidence
    // ------------------------------------------------------------------

    #[test]
    fn evidence_parser_match_is_verified() {
        // The evidence read returned the text we sent (modulo whitespace).
        let ev = parse_evidence_output("hello world", "hello world");
        assert_eq!(ev, SendEvidence::Verified);
    }

    #[test]
    fn evidence_parser_collapses_whitespace() {
        // A leading/trailing newline (the AppleScript `return` adds one)
        // and doubled spaces must still compare as verified.
        let ev = parse_evidence_output("  hello   world\n", "hello world");
        assert_eq!(ev, SendEvidence::Verified);
    }

    #[test]
    fn evidence_parser_mismatch_is_unverified() {
        let ev = parse_evidence_output("some other text", "hello world");
        match ev {
            SendEvidence::Unverified(reason) => {
                assert!(reason.contains("did not match"), "got: {reason}");
            }
            other => panic!("expected Unverified, got {other:?}"),
        }
    }

    #[test]
    fn evidence_parser_empty_is_unverified() {
        let ev = parse_evidence_output("", "hello world");
        match ev {
            SendEvidence::Unverified(reason) => {
                assert!(reason.contains("no text"), "got: {reason}");
            }
            other => panic!("expected Unverified, got {other:?}"),
        }
    }

    #[test]
    fn evidence_parser_case_sensitive() {
        // "Hello" != "hello" — the contract is case-sensitive.
        let ev = parse_evidence_output("Hello world", "hello world");
        assert!(matches!(ev, SendEvidence::Unverified(_)));
    }

    #[test]
    fn format_evidence_verified() {
        let out = format_send_evidence("Sara", &SendEvidence::Verified).unwrap();
        assert_eq!(out, "sent message to Sara (verified)");
    }

    #[test]
    fn format_evidence_unverified_includes_reason() {
        let out =
            format_send_evidence("Sara", &SendEvidence::Unverified("no text".into())).unwrap();
        assert_eq!(out, "sent message to Sara (unverified: no text)");
    }

    #[test]
    fn format_evidence_failed_is_err() {
        let out = format_send_evidence("Sara", &SendEvidence::Failed("boom".into()));
        assert_eq!(out.unwrap_err(), "boom");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn evidence_failure_reason_maps_missing_history_api() {
        assert_eq!(
            evidence_failure_reason(
                "osascript exited with exit status: 1: 91:103: syntax error: Expected end of line but found property. (-2741)"
            ),
            "Messages does not expose message history on this macOS"
        );
        assert_eq!(
            evidence_failure_reason("osascript exited with exit status: 1: boom"),
            "osascript exited with exit status: 1: boom"
        );
    }

    /// Live check, never runs by default. Requires a real recipient handle
    /// or contact name; `TELETYPE_LIVE_SMS_TEXT` overrides the default body:
    ///
    /// `TELETYPE_LIVE_SMS_TO='+15551234567' cargo test -p teletype-desktop \
    ///   --lib live_send_message_check -- --ignored --nocapture`
    #[test]
    #[ignore = "live: sends a real iMessage; set TELETYPE_LIVE_SMS_TO first"]
    fn live_send_message_check() {
        let to = std::env::var("TELETYPE_LIVE_SMS_TO").expect("TELETYPE_LIVE_SMS_TO");
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let text = std::env::var("TELETYPE_LIVE_SMS_TEXT")
            .unwrap_or_else(|_| format!("Teletype live check {secs}"));
        eprintln!("sending to {to}: {text}");
        let c = cmd("send_message", json!({"to": to, "text": text}));
        let result = execute(&c, &ctx_default());
        eprintln!("result: {result:?}");
        assert!(result.is_ok(), "send failed: {result:?}");
    }

    /// Live read-only check: resolves a contact name through Contacts and
    /// sends nothing.
    ///
    /// `TELETYPE_LIVE_CONTACT='Ali Husain Sorathiya' cargo test -p teletype-desktop \
    ///   --lib live_resolve_contact_check -- --ignored --nocapture`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "live: reads the real Contacts database"]
    fn live_resolve_contact_check() {
        let name = std::env::var("TELETYPE_LIVE_CONTACT").expect("TELETYPE_LIVE_CONTACT");
        let result = resolve_contact_handle(&name);
        eprintln!("resolved {name:?} -> {result:?}");
        assert!(result.is_ok(), "lookup failed: {result:?}");
    }

    #[test]
    fn messages_evidence_script_is_escaped() {
        // The `to` argument goes through the same AppleScript escaping as
        // the send script, so a quote in a contact name cannot break out.
        let script = messages_evidence_script("Sa\"ra");
        assert!(
            script.contains("participant \"Sa\\\"ra\" of targetService"),
            "escaping failed: {script}"
        );
    }

    // ------------------------------------------------------------------
    // W1: window identity
    // ------------------------------------------------------------------

    #[test]
    fn same_window_both_titles_some_match() {
        let a = WindowIdentity {
            app_id: "com.a".into(),
            title: Some("Doc".into()),
        };
        let b = WindowIdentity {
            app_id: "com.a".into(),
            title: Some("Doc".into()),
        };
        assert!(same_window(&a, &b));
    }

    #[test]
    fn same_window_both_titles_some_mismatch() {
        let a = WindowIdentity {
            app_id: "com.a".into(),
            title: Some("Doc".into()),
        };
        let b = WindowIdentity {
            app_id: "com.a".into(),
            title: Some("Other".into()),
        };
        assert!(!same_window(&a, &b));
    }

    #[test]
    fn same_window_app_mismatch_wins() {
        // Even with no titles, a different app id is a mismatch.
        let a = WindowIdentity {
            app_id: "com.a".into(),
            title: None,
        };
        let b = WindowIdentity {
            app_id: "com.b".into(),
            title: None,
        };
        assert!(!same_window(&a, &b));
    }

    #[test]
    fn same_window_app_only_allowed_when_both_titles_none() {
        // Both titles None → app-only identity is allowed.
        let a = WindowIdentity {
            app_id: "com.a".into(),
            title: None,
        };
        let b = WindowIdentity {
            app_id: "com.a".into(),
            title: None,
        };
        assert!(same_window(&a, &b));
    }

    #[test]
    fn same_window_empty_app_id_is_not_a_mismatch() {
        // When either app id is empty, the app-id rule is skipped (the
        // caller decides whether that is "identity unavailable").
        let a = WindowIdentity {
            app_id: "".into(),
            title: None,
        };
        let b = WindowIdentity {
            app_id: "com.a".into(),
            title: None,
        };
        assert!(same_window(&a, &b));
    }

    #[test]
    fn same_window_one_title_some_other_none_is_allowed() {
        // Mixed: one side has a title, the other doesn't. The rule only
        // compares when both are Some, so this is allowed; the type_text
        // result string reports it as partial ("window title read once")
        // rather than a full window verification.
        let a = WindowIdentity {
            app_id: "com.a".into(),
            title: Some("Doc".into()),
        };
        let b = WindowIdentity {
            app_id: "com.a".into(),
            title: None,
        };
        assert!(same_window(&a, &b));
    }

    // ------------------------------------------------------------------
    // W1: compose_email URL builder
    // ------------------------------------------------------------------

    #[test]
    fn gmail_url_encodes_ampersand_space_unicode() {
        // & and spaces and unicode must all be encoded in the query
        // string; otherwise a body containing &su=... would inject extra
        // parameters. form_urlencoded::byte_serialize uses form encoding
        // (space -> '+', '&' -> '%26', '@' -> '%40'); a body containing a
        // literal '&su=' must become '%26su=' so it can't inject a param.
        let url = build_gmail_compose_url("a@b.com", "Hi & there", "émoji 🎉", None).unwrap();
        assert!(
            url.starts_with("https://mail.google.com/mail/?view=cm&fs=1&to=a%40b.com"),
            "got: {url}"
        );
        assert!(
            url.contains("su=Hi+%26+there"),
            "ampersand+space not encoded: {url}"
        );
        // 'é' is C3 A9 in UTF-8 -> %C3%A9; '🎉' is F0 9F 8E 89 ->
        // %F0%9F%8E%89.
        assert!(
            url.contains("body=%C3%A9moji+%F0%9F%8E%89"),
            "é/🎉 not encoded: {url}"
        );
        assert!(!url.contains("cc="), "no cc → no cc param: {url}");
        // A body containing a fake parameter must NOT create one.
        let url2 = build_gmail_compose_url("a@b.com", "s", "x&su=evil", None).unwrap();
        assert!(
            !url2.contains("su=evil"),
            "body '&' must not inject a param: {url2}"
        );
    }

    #[test]
    fn gmail_url_includes_cc_when_present() {
        let url = build_gmail_compose_url("a@b.com", "s", "b", Some("c@d.com")).unwrap();
        assert!(url.ends_with("&cc=c%40d.com"), "got: {url}");
    }

    #[test]
    fn gmail_url_empty_to_rejected() {
        assert!(build_gmail_compose_url("", "s", "b", None).is_err());
    }

    #[test]
    fn gmail_url_empty_cc_rejected() {
        assert!(build_gmail_compose_url("a@b.com", "s", "b", Some("")).is_err());
    }

    #[test]
    fn validate_email_addr_rejects_bad_shapes() {
        assert!(validate_email_addr("to", "a@b.com").is_ok());
        assert!(validate_email_addr("to", "a@b.com,c@d.com").is_ok());
        assert!(validate_email_addr("to", "").is_err(), "empty");
        assert!(validate_email_addr("to", "a b@c.com").is_err(), "space");
        assert!(validate_email_addr("to", "a\nb@c.com").is_err(), "newline");
        assert!(validate_email_addr("to", "a@@b.com").is_err(), "two @");
        assert!(validate_email_addr("to", "abc.com").is_err(), "no @");
    }

    // ------------------------------------------------------------------
    // W1: create_calendar_event URL builder
    // ------------------------------------------------------------------

    #[test]
    fn gcal_url_basic() {
        let url = build_gcal_template_url(
            "Standup",
            "2026-01-15T09:00",
            "2026-01-15T09:30",
            None,
            None,
        )
        .unwrap();
        assert!(
            url == "https://calendar.google.com/calendar/render?action=TEMPLATE&text=Standup&dates=20260115T090000/20260115T093000",
            "got: {url}"
        );
    }

    #[test]
    fn gcal_url_encodes_title_and_includes_location_details() {
        let url = build_gcal_template_url(
            "Team & standup",
            "2026-01-15T09:00",
            "2026-01-15T09:30",
            Some("Room 4"),
            Some("Notes & links"),
        )
        .unwrap();
        assert!(
            url.contains("text=Team+%26+standup"),
            "title not encoded: {url}"
        );
        assert!(
            url.contains("&location=Room+4"),
            "location not encoded: {url}"
        );
        assert!(
            url.contains("&details=Notes+%26+links"),
            "details not encoded: {url}"
        );
    }

    #[test]
    fn gcal_url_rejects_bad_month() {
        assert!(
            build_gcal_template_url("t", "2026-13-15T09:00", "2026-13-15T10:00", None, None)
                .is_err()
        );
    }

    #[test]
    fn gcal_url_rejects_bad_day() {
        assert!(
            build_gcal_template_url("t", "2026-01-32T09:00", "2026-01-32T10:00", None, None)
                .is_err()
        );
    }

    #[test]
    fn gcal_url_rejects_bad_hour() {
        assert!(
            build_gcal_template_url("t", "2026-01-15T24:00", "2026-01-15T25:00", None, None)
                .is_err()
        );
    }

    #[test]
    fn gcal_url_rejects_bad_minute() {
        assert!(
            build_gcal_template_url("t", "2026-01-15T09:60", "2026-01-15T10:00", None, None)
                .is_err()
        );
    }

    #[test]
    fn gcal_url_rejects_end_before_start() {
        // Same start/end is not "after"; end before start is also not.
        assert!(
            build_gcal_template_url("t", "2026-01-15T09:00", "2026-01-15T09:00", None, None)
                .is_err()
        );
        assert!(
            build_gcal_template_url("t", "2026-01-15T10:00", "2026-01-15T09:00", None, None)
                .is_err()
        );
    }

    #[test]
    fn gcal_url_rejects_malformed_shape() {
        // Wrong length, wrong separator.
        assert!(parse_local_datetime("2026-1-15T09:00").is_err());
        assert!(parse_local_datetime("202601150900").is_err());
        assert!(parse_local_datetime("2026-01-15 09:00").is_err());
    }

    #[test]
    fn gcal_url_rejects_non_ascii_datetime_without_panicking() {
        // 16 bytes, but a multibyte character straddles the 0..4 slice.
        // Byte indexing used to panic here; the ASCII guard must reject it.
        let weird = "123é-01-15T09:0";
        assert_eq!(weird.len(), 16, "test string must be 16 bytes");
        assert!(parse_local_datetime(weird).is_err());
        assert!(build_gcal_template_url("t", weird, "2026-01-15T10:00", None, None).is_err());
    }

    #[test]
    fn parse_local_datetime_rejects_signed_year() {
        // `"+123"` and `"-123"` are four characters and parse as i32, so the
        // digit guard — not the parse — is what rejects them.
        assert!(parse_local_datetime("+123-01-15T09:00").is_err());
        assert!(parse_local_datetime("-123-01-15T09:00").is_err());
        assert!(parse_local_datetime("0000-01-15T09:00").is_err());
    }

    #[test]
    fn compose_email_requires_subject_and_body() {
        let missing_subject = cmd("compose_email", json!({"to": "a@b.com", "body": "b"}));
        let err = execute_compose_email(&missing_subject).unwrap_err();
        assert!(err.contains("missing 'subject'"), "unexpected error: {err}");

        let missing_body = cmd("compose_email", json!({"to": "a@b.com", "subject": "s"}));
        let err = execute_compose_email(&missing_body).unwrap_err();
        assert!(err.contains("missing 'body'"), "unexpected error: {err}");
    }

    // ------------------------------------------------------------------
    // W1: open_file path resolver
    // ------------------------------------------------------------------

    #[test]
    fn open_file_alias_home() {
        let p = resolve_open_file_path("home").unwrap();
        let home = std::env::var("HOME").unwrap();
        assert_eq!(p, std::path::PathBuf::from(home));
    }

    #[test]
    fn open_file_alias_downloads() {
        let p = resolve_open_file_path("downloads").unwrap();
        let home = std::env::var("HOME").unwrap();
        assert_eq!(p, std::path::PathBuf::from(home).join("Downloads"));
    }

    #[test]
    fn open_file_alias_case_insensitive() {
        let p = resolve_open_file_path("DESKTOP").unwrap();
        let home = std::env::var("HOME").unwrap();
        assert_eq!(p, std::path::PathBuf::from(home).join("Desktop"));
    }

    #[test]
    fn open_file_tilde_expands() {
        let p = resolve_open_file_path("~/Documents").unwrap();
        let home = std::env::var("HOME").unwrap();
        assert_eq!(p, std::path::PathBuf::from(home).join("Documents"));
    }

    #[test]
    fn open_file_bare_tilde() {
        let p = resolve_open_file_path("~").unwrap();
        let home = std::env::var("HOME").unwrap();
        assert_eq!(p, std::path::PathBuf::from(home));
    }

    #[test]
    fn open_file_absolute_passes_through() {
        let p = resolve_open_file_path("/tmp/foo.txt").unwrap();
        assert_eq!(p, std::path::PathBuf::from("/tmp/foo.txt"));
    }

    #[test]
    fn open_file_relative_rejected() {
        assert!(resolve_open_file_path("Documents/foo.txt").is_err());
    }

    #[test]
    fn open_file_empty_rejected() {
        assert!(resolve_open_file_path("").is_err());
    }

    // ------------------------------------------------------------------
    // W1: per-tool control-char exception
    // ------------------------------------------------------------------

    #[test]
    fn control_chars_allowed_in_type_text_text() {
        // \n and \t are legitimate in type_text.text.
        let c = cmd("type_text", json!({"text": "line1\nline2\ttabbed"}));
        assert!(
            reject_control_characters(&c).is_ok(),
            "newline/tab must pass"
        );
    }

    #[test]
    fn control_chars_allowed_in_compose_email_body() {
        let c = cmd(
            "compose_email",
            json!({
                "to": "a@b.com",
                "subject": "s",
                "body": "line1\nline2\tindented"
            }),
        );
        assert!(
            reject_control_characters(&c).is_ok(),
            "newline/tab must pass"
        );
    }

    #[test]
    fn control_chars_still_rejected_in_send_message() {
        // The exception is per (tool, field): send_message.text still
        // rejects newlines (AppleScript injection vector).
        let c = cmd(
            "send_message",
            json!({"to": "Sara", "text": "hi\nrm -rf ~"}),
        );
        let err = reject_control_characters(&c).unwrap_err();
        assert!(err.contains("control characters"), "got: {err}");
    }

    #[test]
    fn control_chars_still_rejected_in_compose_email_to() {
        // Only `body` gets the exception; `to` stays strict.
        let c = cmd(
            "compose_email",
            json!({
                "to": "a@b.com\nc@d.com",
                "subject": "s",
                "body": "b"
            }),
        );
        let err = reject_control_characters(&c).unwrap_err();
        assert!(err.contains("'to'"), "got: {err}");
    }

    #[test]
    fn control_chars_other_controls_still_rejected_in_type_text() {
        // Only \n and \t are allowed; a bare \u0007 (bell) in type_text
        // must still be rejected.
        let c = cmd("type_text", json!({"text": "hi\u{0007}there"}));
        let err = reject_control_characters(&c).unwrap_err();
        assert!(err.contains("control characters"), "got: {err}");
    }

    // ------------------------------------------------------------------
    // W1: type_text size limits
    // ------------------------------------------------------------------

    #[test]
    fn type_text_empty_rejected() {
        // On macOS the handler rejects an empty `text` before any
        // clipboard/keystroke work. We can only test this on macOS; on
        // other platforms the tool is "not supported".
        let c = cmd("type_text", json!({"text": ""}));
        let ctx = ctx_default();
        let result = execute(&c, &ctx);
        #[cfg(target_os = "macos")]
        {
            let err = result.unwrap_err();
            assert!(err.contains("empty"), "got: {err}");
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(result.is_err(), "non-mac type_text is unsupported");
        }
    }

    #[test]
    fn type_text_oversized_rejected() {
        let big = "a".repeat(TYPE_TEXT_MAX_CHARS + 1);
        let c = cmd("type_text", json!({"text": big}));
        let ctx = ctx_default();
        let result = execute(&c, &ctx);
        #[cfg(target_os = "macos")]
        {
            let err = result.unwrap_err();
            assert!(err.contains("max"), "got: {err}");
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(result.is_err(), "non-mac type_text is unsupported");
        }
    }
}
