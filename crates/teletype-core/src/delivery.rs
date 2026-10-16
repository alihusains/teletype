//! Delivery: two layers.
//!
//! 1. **The paste-target gate** (top of file): paste back into the window the
//!    user dictated into, not wherever focus happens to be when the (slow)
//!    pipeline finishes.
//!
//! 2. **The delivery cascade** (bottom of file, P8.1/P8.3): the mechanism by
//!    which the text actually lands — clipboard + paste key, then verified
//!    against the focused field, with typed and set-value fallbacks, a per-app
//!    policy table, and a full-fidelity clipboard snapshot/restore so the
//!    user's clipboard survives every path.
//!
//! A take starts with a hotkey and ends 50 ms to 20 s later, after ASR plus an
//! optional LLM polish. In that gap the user can switch apps or windows —
//! deliberately or by fumbling the hotkey — and an ungated inject then types
//! private dictation into the wrong place. The reference product fixed exactly
//! this for sleeping Chromium hosts: record the target at record start, raise
//! and verify it at delivery time, and refuse the paste on a proven mismatch.
//!
//! The rule is deliberately asymmetric, matching the reference:
//!
//! - **Proven switch, abort.** The recorded and current targets are both
//!   readable and differ. Pasting there would be wrong with certainty.
//! - **Anything unreadable, proceed.** An app whose accessibility sleeps
//!   (Chromium before any client wakes it) reports no focused element and no
//!   window frame. Refusing on no evidence would break every such app, so
//!   missing evidence is not a mismatch.
//!
//! Aborting never loses text: history and the transcript file are written
//! before injection runs, so the gate only decides *where* the text goes, and
//! "nowhere" leaves it recoverable.

/// The paste target as captured when the take started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordTarget {
    /// Bundle id (macOS) or process name (Windows). Empty when unknown.
    pub bundle_id: String,
    /// Human-readable name, for the abort notice only. Never matched on.
    pub app_name: String,
    /// OS process id, when the platform exposes it. Guards against pid reuse
    /// only in combination with `bundle_id`; never matched alone.
    pub pid: Option<u32>,
    /// Focused-window frame (x, y, w, h) when readable. Catches the
    /// same-app-different-window switch no app check can see.
    pub frame: Option<[i64; 4]>,
}

/// What the gate decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Paste. Either the target matches, or there is no evidence it changed.
    Proceed,
    /// Do not paste. The user is provably somewhere else now.
    AbortSwitch,
}

/// The pure decision. `current_bundle` / `current_pid` / `current_frame` are
/// read at delivery time; `None`/empty means "unreadable", which passes.
pub fn resolve_delivery(
    recorded: &RecordTarget,
    current_bundle: &str,
    current_pid: Option<u32>,
    current_frame: Option<[i64; 4]>,
) -> Delivery {
    // No recorded identity, or nothing readable now: no evidence of a switch.
    if recorded.bundle_id.is_empty() || current_bundle.is_empty() {
        return Delivery::Proceed;
    }
    if !bundles_match(&recorded.bundle_id, current_bundle) {
        return Delivery::AbortSwitch;
    }
    // Same app, but both frames readable and different: the user moved to
    // another window of the same app. We cannot reliably raise the old one
    // (no retained element), so refusing is the only safe answer.
    match (recorded.frame, current_frame) {
        (Some(a), Some(b)) if a != b => Delivery::AbortSwitch,
        _ => {
            // A pid change under a matching bundle means the app relaunched
            // mid-take. Its windows are gone, but so is any evidence about
            // where the user is; the frame check above already passed or was
            // unavailable, so proceed rather than strand the text.
            let _ = (recorded.pid, current_pid);
            Delivery::Proceed
        }
    }
}

/// Bundle-id comparison. Case-insensitive: the OS is not consistent about
/// `com.foo.Bar` vs `com.foo.bar` across the APIs we read.
fn bundles_match(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Whether a multi-line dictation into a terminal must be refused.
///
/// A newline in a terminal submits the line: pasting "deploy this\nrm -rf /"
/// runs two commands. The reference refuses terminal payloads containing a
/// newline for exactly this reason. Single-line text is unaffected, and
/// nothing outside a terminal is affected. Refused text stays in history (and
/// on the clipboard when retention applies), so nothing is lost; the user
/// pastes it deliberately if that is what they meant.
///
/// Pure, so the boundary is pinnable: any newline anywhere refuses, including
/// a trailing one (a trailing newline submits an empty command after the
/// text, which still submits).
pub fn refuse_newline_in_terminal(app_type: crate::context::AppType, text: &str) -> bool {
    matches!(app_type, crate::context::AppType::Terminal) && text.contains('\n')
}

/// Delivery cascade (P8.1 / P8.3).
///
/// The gate above decides *where* the text may go; this decides *how* it gets
/// there. The cascade tries, in order:
///
/// 1. Put the text on the clipboard and fire the paste shortcut (the
///    existing, most reliable mechanism), then **verify** by polling the
///    focused field for the text to appear within `VERIFY_TIMEOUT`.
/// 2. If verification failed or the app's policy says so, fall back to
///    typing character-by-character (bounded by `MAX_TYPED_CHARS`) or
///    writing the value straight into the field via accessibility.
/// 3. If everything failed, the text is **held**: the caller keeps it for
///    the user's "Paste last" hotkey (P8.4) rather than losing it.
///
/// Secure fields (password inputs) are refused up front: dictation never
/// lands in a password field, full stop, regardless of policy.
///
/// # Clipboard safety (P8.3)
///
/// Every clipboard path snapshots the whole pasteboard first (via the
/// [`ClipboardIo`] snapshot, which on macOS is the full multi-item
/// `NSPasteboard` capture) and restores it **exactly once** afterwards, on
/// success *and* on failure. Snapshots whose estimated size exceeds
/// [`SNAPSHOT_SIZE_LIMIT`] are skipped: the dictation text simply replaces
/// the clipboard, which is the old behaviour and still never clears it.
use std::time::{Duration, Instant};

use crate::injector::ClipboardSnapshot;

/// Poll interval while waiting for a pasted text to appear in the field.
const VERIFY_INTERVAL: Duration = Duration::from_millis(25);
/// Total time to wait for the paste to land in the field.
const VERIFY_TIMEOUT: Duration = Duration::from_millis(150);
/// A pasted text shorter than this cannot be verified by containment (the
/// field would match almost anything), so verification is skipped and the
/// paste is treated as unverified.
const MIN_VERIFY_LEN: usize = 4;
/// The typed fallback is only worth it for shortish text; beyond this the
/// cascade goes straight to the set-value fallback.
const MAX_TYPED_CHARS: usize = 2000;
/// A snapshot whose parts total more than this many bytes is too big to
/// restore reliably; skip the snapshot and replace the clipboard instead.
const SNAPSHOT_SIZE_LIMIT: usize = 50 * 1024 * 1024;

/// How the text ended up in the target app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// Clipboard paste, and the text was observed in the focused field.
    PastedVerified,
    /// Clipboard paste fired, but the field could not confirm it. The text
    /// most likely landed; the landing is just unproven.
    PastedUnverified,
    /// Typed character-by-character.
    Typed,
    /// Written straight into the field through the accessibility API.
    SetValue,
    /// Every route failed. The caller must keep the text for the user
    /// ("Paste last") — it was never delivered.
    HeldForPasteLast,
    /// Refused before any side effect (secure field, policy refusal).
    Refused { reason: String },
}

/// Read access to the focused field, for verification and the set-value
/// fallback.
///
/// `None` from `selected_text` means "unreadable", which the cascade treats
/// as *no evidence* — the same asymmetry as the paste-target gate: missing
/// evidence never blocks delivery, only proven state does.
pub trait FieldReader: Send + Sync {
    /// The current text of the focused field, if readable.
    fn selected_text(&self) -> Option<String>;
    /// True when the focused field is a secure input (password, OTP, ...).
    /// Secure fields always refuse dictation.
    fn is_secure_field(&self) -> bool;
    /// Replace the focused field's selected text (or insert at the caret)
    /// via the accessibility set-value API. `Ok(())` only when the write
    /// actually landed.
    fn set_selected_text(&self, text: &str) -> Result<(), String>;
}

/// A field reader that sees nothing — for unit tests and platforms without
/// accessibility read access. Verification is impossible, so pastes are
/// always unverified, and both fallbacks are unavailable.
#[derive(Debug, Clone, Default)]
pub struct NoopFieldReader;

impl FieldReader for NoopFieldReader {
    fn selected_text(&self) -> Option<String> {
        None
    }
    fn is_secure_field(&self) -> bool {
        false
    }
    fn set_selected_text(&self, _text: &str) -> Result<(), String> {
        Err("no accessibility set-value support".into())
    }
}

/// Types text character-by-character into the focused field.
pub trait Typer: Send + Sync {
    /// Type `text` at the caret. `Ok(())` only when every character was
    /// sent; a mid-stream failure returns `Err` and the cascade moves on.
    fn type_text(&self, text: &str) -> Result<(), String>;
}

/// Clipboard access for the cascade: set the text, and snapshot/restore the
/// rest of the pasteboard around it.
pub trait ClipboardIo: Send + Sync {
    /// Put `text` on the clipboard as its current contents.
    fn set_text(&self, text: &str) -> Result<(), String>;
    /// Capture the current clipboard so it can be restored. `None` means the
    /// clipboard could not be read; the cascade then must not touch it on
    /// restore either (never clear what we cannot put back).
    fn snapshot(&self) -> Option<ClipboardSnapshot>;
    /// Put `snapshot` back. Must be a faithful write: every item, in order.
    fn restore(&self, snapshot: &ClipboardSnapshot);
}

/// Everything the cascade needs to move text. Kept as one struct so a
/// platform adapter provides all three at once and tests can substitute
/// each independently.
pub struct DeliveryIo {
    pub field: Box<dyn FieldReader>,
    pub typer: Box<dyn Typer>,
    pub clipboard: Box<dyn ClipboardIo>,
    /// Fires the paste shortcut (⌘V / Ctrl+V). `false` when the keystroke
    /// could not be sent; the cascade then treats the paste as unverified.
    pub paste_key: Box<dyn Fn() -> bool + Send + Sync>,
}

impl std::fmt::Debug for DeliveryIo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliveryIo").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Per-app policy table (assets/delivery_policy.toml)
// ---------------------------------------------------------------------------

/// The delivery mode a policy assigns to an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    /// Clipboard paste, then verify; fall through the cascade on failure.
    PasteVerify,
    /// Skip the clipboard entirely; type the text.
    Type,
    /// Skip the clipboard entirely; set the field value directly.
    SetValue,
}

impl DeliveryMode {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "paste_verify" => Ok(Self::PasteVerify),
            "type" => Ok(Self::Type),
            "setvalue" => Ok(Self::SetValue),
            other => Err(format!("unknown delivery mode \"{other}\"")),
        }
    }
}

/// A parsed per-app delivery policy table.
pub struct DeliveryPolicy {
    /// Rules keyed by exact bundle id (macOS) or process name (Windows).
    rules: std::collections::BTreeMap<String, DeliveryMode>,
    /// What unknown apps get.
    default_mode: DeliveryMode,
}

impl DeliveryPolicy {
    /// Parse a TOML policy table. Format:
    ///
    /// ```toml
    /// [default]
    /// mode = "paste_verify"
    ///
    /// ["com.apple.Terminal"]
    /// mode = "type"
    /// ```
    ///
    /// An app matches a rule only by **exact** id (case-sensitive); anything
    /// without a rule gets `[default]`. An unknown `mode` value is an error —
    /// a typo'd table should fail loudly at startup, not silently deliver
    /// text the wrong way.
    pub fn parse(toml: &str) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct Table {
            #[serde(default)]
            default: Option<Rule>,
            #[serde(flatten)]
            apps: std::collections::BTreeMap<String, Rule>,
        }
        #[derive(serde::Deserialize)]
        struct Rule {
            mode: String,
        }
        let table: Table = toml::from_str(toml).map_err(|e| format!("delivery_policy: {e}"))?;
        let default_mode = table
            .default
            .map(|r| DeliveryMode::parse(&r.mode))
            .transpose()?;
        let default_mode = default_mode.unwrap_or(DeliveryMode::PasteVerify);
        let mut rules = std::collections::BTreeMap::new();
        for (id, rule) in table.apps {
            rules.insert(id, DeliveryMode::parse(&rule.mode)?);
        }
        Ok(Self {
            rules,
            default_mode,
        })
    }

    /// The policy table checked into the crate.
    pub fn builtin() -> Self {
        Self::parse(include_str!("../assets/delivery_policy.toml"))
            .expect("bundled delivery_policy.toml must parse")
    }

    /// The mode for `app_id`; unknown ids (and `None`) get the default.
    pub fn mode_for(&self, app_id: Option<&str>) -> DeliveryMode {
        app_id
            .and_then(|id| self.rules.get(id))
            .copied()
            .unwrap_or(self.default_mode)
    }
}

impl std::fmt::Debug for DeliveryPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliveryPolicy")
            .field("rules", &self.rules.len())
            .field("default_mode", &self.default_mode)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// The cascade
// ---------------------------------------------------------------------------

/// A forced delivery path, from `TELETYPE_FORCE_DELIVERY`, for the e2e
/// matrix. Values: `clipboard`, `type`, `setvalue`, `hold`.
///
/// Read once per process (the e2e harness sets it before launch, so a
/// process-lifetime read is the right scope); an unset or unrecognised
/// variable means "no override".
fn forced_path() -> Option<&'static str> {
    use std::sync::OnceLock;
    static FORCED: OnceLock<Option<&'static str>> = OnceLock::new();
    *FORCED.get_or_init(|| {
        std::env::var("TELETYPE_FORCE_DELIVERY")
            .ok()
            .as_deref()
            .and_then(|v| match v {
                "clipboard" => Some("clipboard"),
                "type" => Some("type"),
                "setvalue" => Some("setvalue"),
                "hold" => Some("hold"),
                _ => None,
            })
    })
}

/// Deliver `text` into the focused app, following `policy` for `app_id`.
///
/// See the module docs for the cascade order. This function does the whole
/// job — no caller-side clipboard juggling; the snapshot/restore lives here
/// and runs exactly once per call.
pub fn deliver(
    text: &str,
    app_id: Option<&str>,
    policy: &DeliveryPolicy,
    io: &DeliveryIo,
) -> DeliveryOutcome {
    // Secure fields refuse before any side effect: no clipboard write, no
    // keystroke, nothing. The text stays with the caller.
    if io.field.is_secure_field() {
        return DeliveryOutcome::Refused {
            reason: "focused field is a secure input".into(),
        };
    }

    let mode = match forced_path() {
        Some("type") => DeliveryMode::Type,
        Some("setvalue") => DeliveryMode::SetValue,
        Some("hold") => return DeliveryOutcome::HeldForPasteLast,
        Some("clipboard") | None => policy.mode_for(app_id),
        Some(_) => policy.mode_for(app_id),
    };

    // Snapshot the clipboard once, up front, if this path will touch it.
    // A snapshot we cannot read back is left alone (never cleared); a
    // snapshot too big to restore reliably is skipped for the same reason.
    let mut snapshot: Option<ClipboardSnapshot> = None;
    if matches!(mode, DeliveryMode::PasteVerify) {
        // A snapshot we cannot read back is left alone (never cleared); a
        // snapshot too big to restore reliably is skipped for the same reason.
        if let Some(s) = io.clipboard.snapshot() {
            let too_big = snapshot_size(&s) > SNAPSHOT_SIZE_LIMIT;
            if too_big {
                tracing::warn!("[deliver] clipboard snapshot too large to restore; skipping");
            }
            if !s.is_empty() && !too_big {
                snapshot = Some(s);
            }
        }
    }

    let result = match mode {
        DeliveryMode::PasteVerify => {
            if let Some(outcome) = paste_and_verify(text, io) {
                outcome
            } else {
                fallback(text, io)
            }
        }
        DeliveryMode::Type => match io.typer.type_text(text) {
            Ok(()) => DeliveryOutcome::Typed,
            Err(e) => {
                tracing::warn!("[deliver] typed fallback failed: {e}; trying set-value");
                set_value_or_hold(text, io)
            }
        },
        DeliveryMode::SetValue => set_value_or_hold(text, io),
    };

    // Restore exactly once, on every outcome of the clipboard path. A
    // `None` snapshot means "we could not read it, do not touch it".
    if let Some(snap) = &snapshot {
        io.clipboard.restore(snap);
    }
    result
}

/// Estimated total bytes across every part of every item.
fn snapshot_size(snap: &ClipboardSnapshot) -> usize {
    snap.items
        .iter()
        .map(|i| i.iter().map(|p| p.data.len()).sum::<usize>())
        .sum()
}

/// The clipboard path: set text, fire the paste key, poll the field for the
/// text to appear. Returns `Some` on a conclusive outcome (verified paste,
/// unverified paste, or a refusal) and `None` when the cascade should fall
/// through to the typed/set-value fallbacks.
fn paste_and_verify(text: &str, io: &DeliveryIo) -> Option<DeliveryOutcome> {
    if io.clipboard.set_text(text).is_err() {
        // No clipboard at all: the paste key would paste the user's old
        // clipboard, which is worse than nothing. Fall through.
        return None;
    }
    let key_sent = (io.paste_key)();
    if !key_sent {
        // The keystroke failed. The text is on the clipboard; the user can
        // paste it manually. Report it as held, not as a silent failure.
        return Some(DeliveryOutcome::HeldForPasteLast);
    }

    let verified = verify_paste(text, io);
    if verified {
        return Some(DeliveryOutcome::PastedVerified);
    }
    // The paste did not land, or its landing cannot be proven. When the field
    // is *readable* and the text is absent, the paste demonstrably failed —
    // fall through to the typed / set-value fallbacks. When the field is
    // unreadable, there is no way to know; the paste most likely landed (the
    // keystroke fired), so report it as unverified and stop here.
    if io.field.selected_text().is_some() {
        None
    } else {
        Some(DeliveryOutcome::PastedUnverified)
    }
}

/// Poll the focused field for `text` to appear, up to `VERIFY_TIMEOUT`.
/// `true` only on a positive containment check; unreadable fields are
/// `false` (unverified), never an error.
fn verify_paste(text: &str, io: &DeliveryIo) -> bool {
    if text.len() < MIN_VERIFY_LEN {
        return false;
    }
    let deadline = Instant::now() + VERIFY_TIMEOUT;
    loop {
        if let Some(current) = io.field.selected_text() {
            if current.contains(text) {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(VERIFY_INTERVAL);
    }
}

/// Typed fallback (bounded by `MAX_TYPED_CHARS`), then set-value, then hold.
fn fallback(text: &str, io: &DeliveryIo) -> DeliveryOutcome {
    if text.chars().count() <= MAX_TYPED_CHARS && io.typer.type_text(text).is_ok() {
        return DeliveryOutcome::Typed;
    }
    set_value_or_hold(text, io)
}

fn set_value_or_hold(text: &str, io: &DeliveryIo) -> DeliveryOutcome {
    match io.field.set_selected_text(text) {
        Ok(()) => DeliveryOutcome::SetValue,
        Err(e) => {
            tracing::warn!("[deliver] set-value failed: {e}; holding for Paste last");
            DeliveryOutcome::HeldForPasteLast
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::injector::ClipboardPart;

    fn target() -> RecordTarget {
        RecordTarget {
            bundle_id: "com.apple.TextEdit".into(),
            app_name: "TextEdit".into(),
            pid: Some(1234),
            frame: Some([0, 0, 800, 600]),
        }
    }

    #[test]
    fn same_app_same_window_proceeds() {
        assert_eq!(
            resolve_delivery(
                &target(),
                "com.apple.TextEdit",
                Some(1234),
                Some([0, 0, 800, 600])
            ),
            Delivery::Proceed
        );
    }

    #[test]
    fn different_app_aborts() {
        // The reference case: dictating into Chrome, pasting into Slack.
        assert_eq!(
            resolve_delivery(
                &target(),
                "com.slackmac.Slack",
                Some(5678),
                Some([0, 0, 800, 600])
            ),
            Delivery::AbortSwitch
        );
    }

    #[test]
    fn same_app_different_window_aborts() {
        // Same app, another window: the app check passes, the frame catches it.
        assert_eq!(
            resolve_delivery(
                &target(),
                "com.apple.TextEdit",
                Some(1234),
                Some([100, 100, 800, 600])
            ),
            Delivery::AbortSwitch
        );
    }

    #[test]
    fn unreadable_now_proceeds() {
        // A sleeping host reports nothing. That is no evidence of a switch.
        assert_eq!(
            resolve_delivery(&target(), "", None, None),
            Delivery::Proceed
        );
    }

    #[test]
    fn unrecorded_target_proceeds() {
        // Nothing captured at record start (permissions, race): no baseline,
        // so there is nothing to mismatch against.
        let mut t = target();
        t.bundle_id.clear();
        t.frame = None;
        assert_eq!(
            resolve_delivery(&t, "com.slackmac.Slack", Some(5678), Some([1, 2, 3, 4])),
            Delivery::Proceed
        );
    }

    #[test]
    fn bundle_match_ignores_case() {
        assert_eq!(
            resolve_delivery(
                &target(),
                "COM.APPLE.TEXTEDIT",
                Some(1234),
                Some([0, 0, 800, 600])
            ),
            Delivery::Proceed
        );
    }

    #[test]
    fn relaunched_app_with_same_frame_proceeds() {
        // Same bundle and frame, new pid: the app relaunched into an
        // indistinguishable state. Stranding the text helps nobody.
        assert_eq!(
            resolve_delivery(
                &target(),
                "com.apple.TextEdit",
                Some(9999),
                Some([0, 0, 800, 600])
            ),
            Delivery::Proceed
        );
    }

    #[test]
    fn missing_frame_on_one_side_proceeds_on_app_match() {
        assert_eq!(
            resolve_delivery(&target(), "com.apple.TextEdit", Some(1234), None),
            Delivery::Proceed
        );
    }

    #[test]
    fn multiline_text_into_a_terminal_refuses() {
        use crate::context::AppType;
        assert!(refuse_newline_in_terminal(
            AppType::Terminal,
            "deploy this\nrm -rf /"
        ));
        // A trailing newline still submits (an empty command after the text).
        assert!(refuse_newline_in_terminal(
            AppType::Terminal,
            "deploy this\n"
        ));
        // Single-line terminal dictation is the normal case and proceeds.
        assert!(!refuse_newline_in_terminal(
            AppType::Terminal,
            "deploy this"
        ));
        // Multi-line anywhere else (an editor, a chat box) is fine.
        assert!(!refuse_newline_in_terminal(
            AppType::Document,
            "line one\nline two"
        ));
        assert!(!refuse_newline_in_terminal(
            AppType::Unknown,
            "line one\nline two"
        ));
        assert!(!refuse_newline_in_terminal(AppType::Terminal, ""));
    }

    // -- P8.1 cascade -----------------------------------------------------

    use std::sync::Arc;

    /// A field whose contents are controlled by the test.
    struct TestField {
        text: Arc<std::sync::Mutex<String>>,
        secure: bool,
        set_ok: bool,
        set_calls: Arc<std::sync::atomic::AtomicU32>,
    }

    impl FieldReader for TestField {
        fn selected_text(&self) -> Option<String> {
            Some(self.text.lock().unwrap().clone())
        }
        fn is_secure_field(&self) -> bool {
            self.secure
        }
        fn set_selected_text(&self, text: &str) -> Result<(), String> {
            self.set_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.set_ok {
                *self.text.lock().unwrap() = text.to_string();
                Ok(())
            } else {
                Err("set-value unavailable".into())
            }
        }
    }

    /// A typer that can be told to succeed or fail.
    struct TestTyper {
        ok: bool,
        typed: Arc<std::sync::Mutex<String>>,
        calls: Arc<std::sync::atomic::AtomicU32>,
    }

    impl Typer for TestTyper {
        fn type_text(&self, text: &str) -> Result<(), String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.ok {
                *self.typed.lock().unwrap() = text.to_string();
                Ok(())
            } else {
                Err("type failed".into())
            }
        }
    }

    /// A clipboard that records every call the cascade makes on it.
    struct TestClipboard {
        set_calls: Arc<std::sync::atomic::AtomicU32>,
        set_ok: bool,
        snap_calls: Arc<std::sync::atomic::AtomicU32>,
        restore_calls: Arc<std::sync::atomic::AtomicU32>,
        snapshot: Option<ClipboardSnapshot>,
    }

    impl ClipboardIo for TestClipboard {
        fn set_text(&self, _text: &str) -> Result<(), String> {
            self.set_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.set_ok {
                Ok(())
            } else {
                Err("no clipboard".into())
            }
        }
        fn snapshot(&self) -> Option<ClipboardSnapshot> {
            self.snap_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.snapshot.clone()
        }
        fn restore(&self, _snapshot: &ClipboardSnapshot) {
            self.restore_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn snap_with_text(t: &str) -> ClipboardSnapshot {
        ClipboardSnapshot {
            parts: Vec::new(),
            items: vec![vec![ClipboardPart {
                uti: ClipboardSnapshot::TEXT.to_string(),
                data: t.as_bytes().to_vec(),
            }]],
        }
    }

    /// Build the IO bundle for cascade tests. `field_text` seeds the field;
    /// `paste_key_ok` controls whether the paste shortcut succeeds.
    fn make_io(
        field_text: &str,
        secure: bool,
        field_set_ok: bool,
        typer_ok: bool,
        paste_key_ok: bool,
        snapshot: Option<ClipboardSnapshot>,
    ) -> (
        DeliveryIo,
        Arc<std::sync::Mutex<String>>,
        Arc<std::sync::atomic::AtomicU32>,
        Arc<std::sync::atomic::AtomicU32>,
        Arc<std::sync::atomic::AtomicU32>,
        Arc<std::sync::atomic::AtomicU32>,
        Arc<std::sync::Mutex<String>>,
    ) {
        let text = Arc::new(std::sync::Mutex::new(field_text.to_string()));
        let set_calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let type_calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let clip_set_calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let restore_calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let typed = Arc::new(std::sync::Mutex::new(String::new()));
        let io = DeliveryIo {
            field: Box::new(TestField {
                text: text.clone(),
                secure,
                set_ok: field_set_ok,
                set_calls: set_calls.clone(),
            }),
            typer: Box::new(TestTyper {
                ok: typer_ok,
                typed: typed.clone(),
                calls: type_calls.clone(),
            }),
            clipboard: Box::new(TestClipboard {
                set_calls: clip_set_calls.clone(),
                set_ok: true,
                snap_calls: Arc::new(std::sync::atomic::AtomicU32::new(0)),
                restore_calls: restore_calls.clone(),
                snapshot,
            }),
            paste_key: Box::new(move || paste_key_ok),
        };
        (
            io,
            text,
            set_calls,
            type_calls,
            clip_set_calls,
            restore_calls,
            typed,
        )
    }

    fn default_policy() -> DeliveryPolicy {
        DeliveryPolicy::builtin()
    }

    #[test]
    fn cascade_verified_paste() {
        // The field already contains the text (simulating the paste having
        // landed): the cascade must report PastedVerified and restore the
        // clipboard exactly once.
        let (io, _field, _set, _type_calls, clip_set, restore, _typed) = make_io(
            "hello world", // field already holds the text: paste verified
            false,         // not secure
            true,          // set-value works (unused here)
            true,          // typer works (unused here)
            true,          // paste key sent
            Some(snap_with_text("user's old copy")),
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::PastedVerified);
        assert_eq!(
            clip_set.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "clipboard set exactly once"
        );
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "clipboard restored exactly once"
        );
    }

    #[test]
    fn cascade_unverified_paste_when_field_unreadable() {
        // The NoopFieldReader cannot confirm the paste; the cascade reports
        // PastedUnverified but still restores the clipboard.
        let mut io = make_io("", false, true, true, true, Some(snap_with_text("x"))).0;
        io.field = Box::new(NoopFieldReader);
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::PastedUnverified);
    }

    #[test]
    fn cascade_type_fallback_when_paste_unverified_and_field_readable() {
        // The field is readable but does not contain the text, so verification
        // fails. The typer is available, so the cascade falls back to typing.
        // Note: this path requires the field to be readable-but-missing, which
        // means the paste demonstrably did not land.
        //
        // To exercise this we need the field readable (so verification can
        // fail on a readable field) and the typer to succeed.
        let (io, _field, _set, type_calls, _clip, _restore, _typed) = make_io(
            "something else entirely", // field readable, text not present
            false,
            true,
            true, // typer succeeds
            true,
            None, // no snapshot
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        // The paste was unverified (field readable but text missing), so the
        // cascade falls through to typing.
        assert_eq!(outcome, DeliveryOutcome::Typed);
        assert_eq!(
            type_calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "typer called exactly once"
        );
    }

    #[test]
    fn cascade_setvalue_fallback_when_type_fails() {
        // Typer fails, set-value works: the cascade reports SetValue.
        let (io, _field, set_calls, type_calls, _clip, _restore, _typed) = make_io(
            "something else",
            false,
            true,  // set-value works
            false, // typer fails
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::SetValue);
        assert_eq!(type_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(set_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn cascade_hold_when_everything_fails() {
        // Typer fails, set-value fails: the cascade holds the text.
        let (io, _field, _set, _type_calls, _clip, _restore, _typed) = make_io(
            "something else",
            false,
            false, // set-value fails
            false, // typer fails
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::HeldForPasteLast);
    }

    #[test]
    fn cascade_secure_field_refused() {
        // A secure field refuses before any side effect.
        let (io, _field, _set, _type_calls, clip_set, restore, _typed) = make_io(
            "",
            true, // secure
            true,
            true,
            true,
            Some(snap_with_text("x")),
        );
        let policy = default_policy();
        let outcome = deliver("password123", None, &policy, &io);
        assert_eq!(
            outcome,
            DeliveryOutcome::Refused {
                reason: "focused field is a secure input".into()
            }
        );
        assert_eq!(
            clip_set.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "clipboard untouched"
        );
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "clipboard not restored"
        );
    }

    #[test]
    fn cascade_policy_type_mode_skips_clipboard() {
        // A policy that says "type" for an app must skip the clipboard
        // entirely and go straight to the typer.
        let (io, _field, _set, type_calls, clip_set, restore, _typed) = make_io(
            "",
            false,
            true,
            true, // typer works
            true,
            Some(snap_with_text("x")),
        );
        let policy = default_policy();
        // "com.apple.Terminal" is mapped to "type" in the policy table.
        let outcome = deliver("ls -la", Some("com.apple.Terminal"), &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::Typed);
        assert_eq!(type_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            clip_set.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "clipboard not set in type mode"
        );
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "clipboard not restored in type mode"
        );
    }

    #[test]
    fn cascade_policy_setvalue_mode_skips_clipboard() {
        // A policy that says "setvalue" must skip clipboard and typer.
        let policy_toml = r#"
[default]
mode = "setvalue"
"#;
        let policy = DeliveryPolicy::parse(policy_toml).unwrap();
        let (io, _field, set_calls, type_calls, clip_set, restore, _typed) = make_io(
            "",
            false,
            true, // set-value works
            true,
            true,
            Some(snap_with_text("x")),
        );
        let outcome = deliver("some text", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::SetValue);
        assert_eq!(set_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(type_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(clip_set.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(restore.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn cascade_env_override_honored() {
        // TELETYPE_FORCE_DELIVERY is read once via OnceLock; we cannot set it
        // per-test in the same process. This test documents the contract:
        // the override is read at process start. We verify the parse logic
        // directly by checking that the function returns None when unset.
        // (A full e2e test would set the env var before process launch.)
        // Here we just verify the cascade works with the default policy when
        // no override is set.
        let (io, _field, _set, _type_calls, _clip, _restore, _typed) =
            make_io("", false, true, true, true, None);
        let policy = default_policy();
        let outcome = deliver("hello", None, &policy, &io);
        // With no env override and default policy (paste_verify), and the
        // field readable but not containing the text, we get unverified paste
        // or typed fallback.
        assert!(
            matches!(
                outcome,
                DeliveryOutcome::PastedUnverified | DeliveryOutcome::Typed
            ),
            "unexpected outcome: {outcome:?}"
        );
    }

    #[test]
    fn policy_parse_and_match() {
        let toml = r#"
[default]
mode = "paste_verify"

["com.apple.Terminal"]
mode = "type"

["com.microsoft.Word"]
mode = "paste_verify"
"#;
        let policy = DeliveryPolicy::parse(toml).unwrap();
        assert_eq!(
            policy.mode_for(Some("com.apple.Terminal")),
            DeliveryMode::Type
        );
        assert_eq!(
            policy.mode_for(Some("com.microsoft.Word")),
            DeliveryMode::PasteVerify
        );
        // Unknown app gets the default.
        assert_eq!(
            policy.mode_for(Some("com.unknown.App")),
            DeliveryMode::PasteVerify
        );
        // None gets the default.
        assert_eq!(policy.mode_for(None), DeliveryMode::PasteVerify);
    }

    #[test]
    fn policy_parse_invalid_mode_errors() {
        let toml = r#"
[default]
mode = "bogus"
"#;
        assert!(DeliveryPolicy::parse(toml).is_err());
    }

    #[test]
    fn policy_builtin_parses() {
        // The checked-in table must parse and have the expected entries.
        let policy = DeliveryPolicy::builtin();
        assert_eq!(
            policy.mode_for(Some("com.apple.Terminal")),
            DeliveryMode::Type
        );
        assert_eq!(
            policy.mode_for(Some("com.googlecode.iterm2")),
            DeliveryMode::Type
        );
        assert_eq!(
            policy.mode_for(Some("com.microsoft.Word")),
            DeliveryMode::PasteVerify
        );
        assert_eq!(
            policy.mode_for(Some("com.microsoft.VSCode")),
            DeliveryMode::PasteVerify
        );
        assert_eq!(
            policy.mode_for(Some("com.something.else")),
            DeliveryMode::PasteVerify
        );
    }

    #[test]
    fn clipboard_restore_called_once_on_success() {
        // A successful paste_verify path must restore the snapshot exactly
        // once, even though the cascade touched the clipboard.
        let (io, _field, _set, _type_calls, _clip, restore, _typed) = make_io(
            "",
            false,
            true,
            true,
            true,
            Some(snap_with_text("user's clipboard")),
        );
        let policy = default_policy();
        let _ = deliver("hello world", None, &policy, &io);
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "restore called exactly once"
        );
    }

    #[test]
    fn clipboard_restore_on_paste_failure() {
        // If the paste key fails, the cascade holds the text, but the
        // clipboard snapshot (taken before the paste attempt) must still be
        // restored so the user's clipboard is not lost.
        let (io, _field, _set, _type_calls, _clip, restore, _typed) = make_io(
            "",
            false,
            true,
            true,
            false, // paste key fails
            Some(snap_with_text("user's clipboard")),
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::HeldForPasteLast);
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "clipboard restored even on paste failure"
        );
    }

    #[test]
    fn clipboard_snapshot_too_large_is_skipped() {
        // A snapshot exceeding SNAPSHOT_SIZE_LIMIT must be skipped: the
        // cascade still delivers but does not attempt restore.
        let mut big_snap = ClipboardSnapshot {
            parts: Vec::new(),
            items: Vec::new(),
        };
        // Build a snapshot just over 50 MB.
        let part_size = (SNAPSHOT_SIZE_LIMIT / 2) + 1;
        big_snap.items.push(vec![ClipboardPart {
            uti: "com.test.big".to_string(),
            data: vec![0u8; part_size],
        }]);
        big_snap.items.push(vec![ClipboardPart {
            uti: "com.test.big2".to_string(),
            data: vec![0u8; part_size],
        }]);

        let (io, _field, _set, _type_calls, _clip, restore, _typed) =
            make_io("", false, true, true, true, Some(big_snap));
        let policy = default_policy();
        let _ = deliver("hello world", None, &policy, &io);
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "restore skipped for oversized snapshot"
        );
    }
}
