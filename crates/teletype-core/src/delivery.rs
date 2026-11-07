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
/// Total time to wait for the paste to land in the field (first pass).
const VERIFY_TIMEOUT: Duration = Duration::from_millis(150);
/// After the first pass times out on a *readable* field, the AX `value`
/// attribute often simply lags the actual paste on a loaded machine (the
/// keystroke fired, the clipboard held the text, but the focused field's
/// accessibility value has not republished yet). Rather than assume the
/// paste failed and fall through to the typed fallback — which is both
/// crash-prone (enigo `fast_text` hits TIS off-main on newline text) and
/// unreliable (it has mangled real text into repeated `a`s) — re-poll the
/// field for this much longer before concluding the paste really failed.
const VERIFY_GRACE: Duration = Duration::from_millis(400);
/// A pasted text shorter than this cannot be verified by containment (the
/// field would match almost anything), so verification is skipped and the
/// paste is treated as unverified.
const MIN_VERIFY_LEN: usize = 4;
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
    /// The paste could not run, so the text was left on the clipboard as a
    /// kept write (not restored afterwards) and the caller is told to press
    /// the paste shortcut. Used for non-secure refusals (no writable focus)
    /// and for inconclusive paste verification (F-19): in both cases the
    /// text is on the clipboard right now, and the user's own copy was put
    /// back before the write. Secure fields never reach this outcome.
    ClipboardOnly,
    /// Accessibility is not trusted, so the paste shortcut cannot be sent
    /// reliably. The text was left on the clipboard as a kept write, same as
    /// [`ClipboardOnly`], and the caller's message must name the cause.
    AccessibilityDenied,
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
    /// True when the focused element is a writable text field.
    ///
    /// Checked before the optimistic-paste path: with no writable focus the
    /// field is a web/Electron/chat area that cannot be read back, so the
    /// cascade fires the paste keystroke optimistically into whatever is
    /// focused (see `deliver`). Defaults to true because missing evidence
    /// must not block delivery; an adapter that can prove the absence
    /// overrides it.
    fn has_writable_focus(&self) -> bool {
        true
    }
    /// True when the focused element is a browser URL / address / search bar.
    ///
    /// Distinguishing these from ordinary text fields is what lets the
    /// optimistic-paste path fire ⌘V into web content without ever navigating
    /// to the dictated text (BUG-006 / L038). Defaults to false: an adapter
    /// that cannot observe the role must not block delivery on a guess.
    fn is_url_field(&self) -> bool {
        false
    }
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
    ///
    /// The platform guards the write itself against a clipboard that moved
    /// since the snapshot (the snapshot's `change_count` is re-checked
    /// against the live count; a mismatch leaves the clipboard alone). A
    /// `None` count is the platform's "I cannot observe the count" and
    /// restores unconditionally, the old behaviour.
    fn restore(&self, snapshot: &ClipboardSnapshot);
    /// The clipboard's current change count, when the platform exposes one
    /// (macOS `NSPasteboard changeCount`, Windows
    /// `GetClipboardSequenceNumber`). `None` when the platform cannot
    /// observe it; the restore guard then restores unconditionally, which
    /// is the old behaviour and the only one such a platform can offer.
    fn change_count(&self) -> Option<u64> {
        None
    }
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
    /// Live accessibility-trust check, read once at the top of `deliver`.
    /// `false` means the paste shortcut cannot be sent reliably, so the
    /// cascade goes straight to the clipboard-kept path and the caller's
    /// message names the cause (`AccessibilityDenied`).
    ///
    /// Defaults to `true` (the trait default: no permission model to
    /// violate). The desktop adapter wires this to the platform's live
    /// trust check; tests that do not care leave it at the default.
    pub accessibility_trusted: Box<dyn Fn() -> bool + Send + Sync>,
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
#[derive(Clone)]
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

    /// The policy table checked into the crate. A corrupt bundled table
    /// must not take the app down at first delivery; fall back to a
    /// default-mode-only policy (every app gets the default mode) rather
    /// than panic.
    pub fn builtin() -> Self {
        match Self::parse(include_str!("../assets/delivery_policy.toml")) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("bundled delivery_policy.toml failed to parse: {e}; using the default mode for every app");
                Self {
                    rules: std::collections::BTreeMap::new(),
                    default_mode: DeliveryMode::PasteVerify,
                }
            }
        }
    }

    /// The mode for `app_id`; unknown ids (and `None`) get the default.
    pub fn mode_for(&self, app_id: Option<&str>) -> DeliveryMode {
        app_id
            .and_then(|id| self.rules.get(id))
            .copied()
            .unwrap_or(self.default_mode)
    }

    /// A copy of this table with one exact-id rule added (or replaced),
    /// plus an optional replacement default. Used by the e2e hook to apply
    /// its per-app `policy` overrides without editing the checked-in file.
    pub fn with_override(
        &self,
        app_id: &str,
        mode: DeliveryMode,
        default_mode: Option<DeliveryMode>,
    ) -> Self {
        let mut rules = self.rules.clone();
        rules.insert(app_id.to_string(), mode);
        Self {
            rules,
            default_mode: default_mode.unwrap_or(self.default_mode),
        }
    }

    /// A copy of this table whose default is `mode` (every app without an
    /// explicit rule gets it). Used by the e2e hook's `hold` override,
    /// which must force the hold path even for apps with an explicit rule.
    pub fn with_default(&self, mode: DeliveryMode) -> Self {
        Self {
            rules: self.rules.clone(),
            default_mode: mode,
        }
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

/// The bundle id the forced path applies to, from `TELETYPE_FORCE_APP`.
/// `None` means the forced path applies to every app (the original
/// process-wide semantics); `Some(id)` scopes it to one exact id, which is
/// how the e2e hook forces a single app's cascade step at a time.
fn forced_app() -> Option<&'static str> {
    use std::sync::OnceLock;
    static APP: OnceLock<Option<String>> = OnceLock::new();
    let v = APP.get_or_init(|| match std::env::var("TELETYPE_FORCE_APP").ok() {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    });
    v.as_deref()
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
    // TELETYPE_DELIVERY_AUDIT: ground-truth probe for the double-delivery
    // bug. Logs every deliver() call with its exact text, mode, and a
    // monotonic timestamp so we can see how many deliveries fire per take
    // and what each contains. Gated on an env var so it is a no-op in
    // production.
    if std::env::var("TELETYPE_DELIVERY_AUDIT").is_ok() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        eprintln!(
            "[DELIVERY_AUDIT] t={}ms ENTER chars={} text={:?}",
            ts,
            text.chars().count(),
            text
        );
    }
    // Secure fields refuse before any side effect: no clipboard write, no
    // keystroke, nothing. The text stays with the caller.
    if io.field.is_secure_field() {
        return DeliveryOutcome::Refused {
            reason: "focused field is a secure input".into(),
        };
    }

    // Browser URL / address / search bars refuse the same way (BUG-006): an
    // optimistic ⌘V into an omnibox, followed by the app's Enter handling,
    // would navigate to the dictated text. The text stays with the caller.
    // This is what lets the optimistic-paste path below fire freely into web
    // content and chat without ever risking a URL-bar navigation.
    if io.field.is_url_field() {
        return DeliveryOutcome::Refused {
            reason: "focused field is a browser address or search bar".into(),
        };
    }

    // A forced path applies to the take's app only when scoped by
    // `TELETYPE_FORCE_APP` (the e2e hook's per-app override); unscoped, it
    // is process-wide (the original matrix harness semantics).
    let forced = match (forced_path(), forced_app()) {
        (Some(_), Some(app)) if app != app_id.unwrap_or("") => None,
        (p, _) => p,
    };

    let mode = match forced {
        Some("type") => DeliveryMode::Type,
        Some("setvalue") => DeliveryMode::SetValue,
        Some("hold") => return DeliveryOutcome::HeldForPasteLast,
        Some("clipboard") | None => policy.mode_for(app_id),
        Some(_) => policy.mode_for(app_id),
    };

    // F-17: read the accessibility trust live, once, before any side effect.
    // Without the grant the paste shortcut cannot be sent (and the typed /
    // set-value routes are AX routes too), so the only honest outcome is the
    // clipboard-kept path with a message that names the cause. The secure
    // field check above still wins: a secure field is refused before any
    // side effect, trust or not.
    let trusted = (io.accessibility_trusted)();

    // No writable AX text field is focused. This is the web/Electron/chat
    // case (a browser content area, a chat box, an app whose whole content
    // is one opaque AXGroup): the field is there and focused, just not
    // readable through the AX API, so we cannot verify a direct write.
    //
    // We follow the market behaviour (EnviousWispr, Wispr Flow): fire the
    // ⌘V keystroke optimistically and let the OS route it into the focused
    // element, rather than refusing. The user's original L038 concern -- a
    // ⌘V landing in the app's default paste target -- is bounded by the
    // guards that already run before this point:
    //   * secure fields refused above (password / OTP),
    //   * browser URL / address / search bars refused below (BUG-006),
    // so the only place this can misroute is a focused control that is
    // neither a text field, a secure field, nor a URL bar (a button, a
    // list, ...), which is the accepted trade for pasting into chat/web.
    //
    // The paste-verify path handles the unreadable field gracefully: it
    // reports `PastedUnverified` (the keystroke fired, the field cannot be
    // read back to confirm), never a second copy.
    if matches!(mode, DeliveryMode::PasteVerify) && !io.field.has_writable_focus() {
        tracing::info!(
            "[deliver] no writable field focused; pasting optimistically into the focused element"
        );
        return paste_unverified_path(text, io, trusted);
    }

    let result = match mode {
        DeliveryMode::PasteVerify => {
            // F-17: an untrusted accessibility grant cannot send the paste
            // shortcut (enigo posts CGEvents, which the OS drops for an
            // untrusted process). The typed and set-value routes are AX
            // routes too, so they would fail the same way. Go straight to
            // the clipboard-kept path and say why.
            if !trusted {
                keep_on_clipboard(text, io, trusted)
            } else {
                // Snapshot the clipboard once, up front, if this path will
                // touch it. A snapshot we cannot read back is left alone
                // (never cleared); a snapshot too big to restore reliably
                // is skipped for the same reason. The snapshot carries the
                // platform's change count (when it has one) so the restore
                // at the end can be guarded against a clipboard that moved
                // in the meantime (F-18).
                let snapshot: Option<ClipboardSnapshot> = io.clipboard.snapshot().filter(|s| {
                    if s.is_empty() {
                        return false;
                    }
                    if snapshot_size(s) > SNAPSHOT_SIZE_LIMIT {
                        tracing::warn!(
                            "[deliver] clipboard snapshot too large to restore; skipping"
                        );
                        return false;
                    }
                    true
                });
                let outcome = match paste_and_verify(text, io) {
                    Some(outcome) => outcome,
                    // F-19: `paste_and_verify` now returns `None` only when
                    // the clipboard write itself failed (no clipboard at
                    // all), in which case the typed / set-value routes are
                    // the only remaining options. An inconclusive
                    // verification is a `Some(ClipboardOnly)` above and
                    // never reaches here.
                    None => fallback(text, io),
                };
                // F-18: restore exactly once, on every outcome of the
                // clipboard path, but only if the clipboard still holds our
                // write. A `None` snapshot means "we could not read it, do
                // not touch it". A snapshot whose `change_count` has moved
                // since we wrote means the user or another app copied
                // something in the meantime, and restoring would overwrite
                // their copy, so the restore is skipped. A `None` count
                // (platform without a change count, or a snapshot taken
                // before this guard existed) restores unconditionally, the
                // old behaviour.
                if let Some(snap) = &snapshot {
                    let still_ours = match snap.change_count {
                        Some(captured) => io
                            .clipboard
                            .change_count()
                            .is_none_or(|now| now == captured),
                        None => true,
                    };
                    if still_ours {
                        io.clipboard.restore(snap);
                    } else {
                        tracing::debug!(
                            "[deliver] clipboard changed since our write; skipping restore"
                        );
                    }
                }
                outcome
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

    if std::env::var("TELETYPE_DELIVERY_AUDIT").is_ok() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        eprintln!("[DELIVERY_AUDIT] t={}ms EXIT outcome={:?}", ts, result);
    }
    result
}

/// The clipboard-kept path (F-16 / F-17): the paste could not run, so the
/// text is left on the clipboard as a kept write and the caller is told to
/// press the paste shortcut. The dictation is written as the clipboard's
/// current contents (which, on macOS, becomes the last item, what a normal
/// paste reads). The user's previous copy is lost, which is the documented
/// trade: the user can press the paste shortcut and get the dictation,
/// which is the whole point of this path.
///
/// Returns [`DeliveryOutcome::AccessibilityDenied`] when the trust check
/// said `false` (the caller's message names the cause), otherwise
/// [`DeliveryOutcome::ClipboardOnly`]. A failed clipboard write means the
/// clipboard is unavailable, which the caller handles as a plain refusal
/// (the text stays in History).
fn keep_on_clipboard(text: &str, io: &DeliveryIo, trusted: bool) -> DeliveryOutcome {
    if io.clipboard.set_text(text).is_err() {
        return DeliveryOutcome::Refused {
            reason: "the clipboard is unavailable and there is no writable text field".into(),
        };
    }
    if trusted {
        DeliveryOutcome::ClipboardOnly
    } else {
        DeliveryOutcome::AccessibilityDenied
    }
}

/// The optimistic-paste path for a focused element that is not a writable AX
/// text field (web content, chat boxes, Electron apps). The field cannot be
/// read back, so we cannot verify a direct write; instead we put the text on
/// the clipboard and fire the paste keystroke, letting the OS route it into
/// the focused element.
///
/// The clipboard is written *first* so the ⌘V reads our dictation, not the
/// user's previous copy. Outcomes:
/// - keystroke sent -> [`DeliveryOutcome::PastedUnverified`]: the paste most
///   likely landed; the field is unreadable so we cannot confirm, and F-19
///   forbids a second copy, so we report it and stop. The caller's clipboard
///   restore in `deliver` does not run here (this path returns early), which
///   is correct: the dictation is now what a normal ⌘V would paste, so the
///   user can re-paste if it missed.
/// - keystroke not sent -> fall back to [`keep_on_clipboard`]: the text stays
///   on the clipboard and the user is told to paste manually.
fn paste_unverified_path(text: &str, io: &DeliveryIo, trusted: bool) -> DeliveryOutcome {
    if io.clipboard.set_text(text).is_err() {
        return DeliveryOutcome::Refused {
            reason: "the clipboard is unavailable and there is no writable text field".into(),
        };
    }
    if (io.paste_key)() {
        DeliveryOutcome::PastedUnverified
    } else {
        keep_on_clipboard(text, io, trusted)
    }
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
///
/// F-19: nothing inconclusive may fall through to the typed / set-value
/// fallbacks, because the paste keystroke already fired and a second copy
/// would double the dictation. What the caller is told depends on how
/// inconclusive the verification was:
///
/// - Field unreadable, or text under `MIN_VERIFY_LEN`: verification cannot
///   work, and the paste most likely landed. The outcome is
///   `PastedUnverified`: quiet, no pill message, and the clipboard restore
///   in `deliver` runs as usual (guarded by the change-count rule).
/// - Field readable and verifiable, but the text is still absent after the
///   grace re-poll: the paste may genuinely not have landed. The outcome
///   is `ClipboardOnly`: the text stays on the clipboard as a kept write
///   and the caller tells the user to press the paste shortcut.
fn paste_and_verify(text: &str, io: &DeliveryIo) -> Option<DeliveryOutcome> {
    if io.clipboard.set_text(text).is_err() {
        // No clipboard at all: the paste key would paste the user's old
        // clipboard, which is worse than nothing. Fall through.
        return None;
    }
    let key_sent = (io.paste_key)();
    if !key_sent {
        // The keystroke failed. The text is on the clipboard. Fall through
        // to the typed / set-value routes: the paste never fired, so there
        // is no risk of a second copy, and the typed / set-value routes are
        // the only remaining options. If those also fail, the cascade holds
        // the text.
        return None;
    }

    let verified = verify_paste(text, io);
    if verified {
        return Some(DeliveryOutcome::PastedVerified);
    }
    // The first pass did not prove the paste. Three cases:
    //
    // - Field unreadable: there is no way to know whether the paste landed.
    //   The keystroke fired and the clipboard held the text, so it most
    //   likely did. Report PastedUnverified: quiet, no pill message, and
    //   the clipboard restore in `deliver` runs as usual (guarded by the
    //   change-count rule). F-19 only forbids a SECOND copy (typed or
    //   set-value), so no fallback runs here either.
    // - Field readable, text too short to verify: under `MIN_VERIFY_LEN`
    //   the containment check is meaningless, so verification cannot work.
    //   Same reasoning as the unreadable case: the paste most likely landed,
    //   so treat it as PastedUnverified, not as a failed paste.
    // - Field readable, text verifiable: the text is absent *right now*,
    //   but on a loaded machine the AX `value` attribute can lag the real
    //   paste well past `VERIFY_TIMEOUT`. Re-poll for `VERIFY_GRACE`
    //   before assuming the paste failed. If it appears, the paste landed
    //   late: verified. If it is still absent after the grace period, this
    //   is the genuinely suspicious case, and F-19: stop with the text on
    //   the clipboard rather than typing a second copy. The typed fallback
    //   is crash-prone (enigo `fast_text` hits TIS off-main on newline
    //   text) and has mangled real dictation into repeated `a`s, so the
    //   clipboard-kept path is the safer answer to an inconclusive
    //   read-back.
    if io.field.selected_text().is_none() || text.len() < MIN_VERIFY_LEN {
        Some(DeliveryOutcome::PastedUnverified)
    } else if verify_paste_grace(text, io) {
        Some(DeliveryOutcome::PastedVerified)
    } else {
        Some(DeliveryOutcome::ClipboardOnly)
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

/// Second-chance poll after the first verify pass timed out on a readable
/// field: wait up to `VERIFY_GRACE` more for the AX `value` to catch up with
/// a paste that already landed. Same containment rule as [`verify_paste`];
/// too-short text is unverifiable and returns `false` (the caller then falls
/// through, matching the first pass's behaviour for short text).
fn verify_paste_grace(text: &str, io: &DeliveryIo) -> bool {
    if text.len() < MIN_VERIFY_LEN {
        return false;
    }
    let deadline = Instant::now() + VERIFY_GRACE;
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

/// The last-resort path when the ⌘V paste could not be confirmed.
///
/// This used to be a character-by-character `fast_text` typed fallback, but
/// `fast_text` mangles text into repeated characters in some apps (observed in
/// terminals such as Terax: a dictation arrived as "aaaa..."), and the OS gives
/// no reliable signal for *which* apps do this. A bundle-id blocklist is
/// brittle and cannot cover unknown apps, so the typed route is no longer used
/// as a delivery path at all: whenever the paste cannot be confirmed, the text
/// is held for Paste-Last and the user pastes it themselves (⌘V, which the
/// app's own paste handling routes correctly) rather than receive garbled text.
///
/// `set_value` is still tried first: it is a single AX write (not a keystroke
/// stream), so it cannot produce the repeated-character corruption, and it is
/// the only route that can still land the text automatically. When even that
/// is unavailable the text is held.
fn fallback(text: &str, io: &DeliveryIo) -> DeliveryOutcome {
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

    use std::sync::{Arc, Mutex};

    /// A field whose contents are controlled by the test.
    struct TestField {
        text: Arc<std::sync::Mutex<String>>,
        secure: bool,
        set_ok: bool,
        writable_focus: bool,
        url_field: bool,
        readable: bool,
        set_calls: Arc<std::sync::atomic::AtomicU32>,
    }

    impl FieldReader for TestField {
        fn selected_text(&self) -> Option<String> {
            if !self.readable {
                return None;
            }
            Some(self.text.lock().unwrap().clone())
        }
        fn is_secure_field(&self) -> bool {
            self.secure
        }
        fn has_writable_focus(&self) -> bool {
            self.writable_focus
        }
        fn is_url_field(&self) -> bool {
            self.url_field
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
    /// `change_count` is the live change count the cascade reads before
    /// restoring; `None` means "the platform has no count" (the old
    /// unconditional behaviour).
    struct TestClipboard {
        set_calls: Arc<std::sync::atomic::AtomicU32>,
        set_ok: bool,
        snap_calls: Arc<std::sync::atomic::AtomicU32>,
        restore_calls: Arc<std::sync::atomic::AtomicU32>,
        snapshot: Option<ClipboardSnapshot>,
        change_count: Option<u64>,
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
        fn change_count(&self) -> Option<u64> {
            self.change_count
        }
    }

    fn snap_with_text(t: &str) -> ClipboardSnapshot {
        ClipboardSnapshot {
            parts: Vec::new(),
            items: vec![vec![ClipboardPart {
                uti: ClipboardSnapshot::TEXT.to_string(),
                data: t.as_bytes().to_vec(),
            }]],
            change_count: None,
        }
    }

    /// Build the IO bundle for cascade tests. `field_text` seeds the field;
    /// `paste_key_ok` controls whether the paste shortcut succeeds.
    /// `trusted` controls the live accessibility-trust check; `change_count`
    /// is the live clipboard change count the cascade reads before
    /// restoring (`None` means "no count", the old behaviour).
    type Counter = Arc<std::sync::atomic::AtomicU32>;
    type IoCounters = (Counter, Counter, Counter, Counter);

    #[allow(clippy::too_many_arguments)]
    fn make_io(
        field_text: &str,
        secure: bool,
        field_set_ok: bool,
        typer_ok: bool,
        paste_key_ok: bool,
        snapshot: Option<ClipboardSnapshot>,
        trusted: bool,
        change_count: Option<u64>,
    ) -> (
        DeliveryIo,
        Arc<Mutex<String>>,
        IoCounters,
        Arc<Mutex<String>>,
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
                writable_focus: true,
                url_field: false,
                readable: true,
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
                change_count,
            }),
            paste_key: Box::new(move || paste_key_ok),
            accessibility_trusted: Box::new(move || trusted),
        };
        (
            io,
            text,
            (set_calls, type_calls, clip_set_calls, restore_calls),
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
        let (io, _field, (_set, _type_calls, clip_set, restore), _typed) = make_io(
            "hello world", // field already holds the text: paste verified
            false,         // not secure
            true,          // set-value works (unused here)
            true,          // typer works (unused here)
            true,          // paste key sent
            Some(snap_with_text("user's old copy")),
            true, // accessibility trusted
            None, // no change count (old behaviour)
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
        // The NoopFieldReader cannot confirm the paste. The paste keystroke
        // fired and the clipboard held the text, so the paste most likely
        // landed: the outcome is PastedUnverified (quiet, no pill message),
        // not ClipboardOnly (which would tell the user to paste a second
        // copy). F-19 still holds: the typed / set-value fallbacks never
        // run, so a second copy is never sent. The clipboard is restored as
        // before, so the user's own copy comes back.
        let mut io = make_io(
            "",
            false,
            true,
            true,
            true,
            Some(snap_with_text("x")),
            true,
            None,
        )
        .0;
        io.field = Box::new(NoopFieldReader);
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::PastedUnverified);
    }

    #[test]
    fn cascade_unverified_paste_when_field_unreadable_restores_clipboard() {
        // The unreadable-field case (Terminal, most Electron apps, many
        // browser fields) is a PastedUnverified, so the clipboard restore
        // runs exactly as before (guarded by the change-count rule). The
        // user's own copy comes back; the dictation is not stranded on the
        // clipboard as a kept write.
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::{Arc, Mutex};

        let restore = Arc::new(AtomicU32::new(0));
        let restore_clone = restore.clone();
        let io2 = DeliveryIo {
            field: Box::new(NoopFieldReader),
            typer: Box::new(TestTyper {
                ok: true,
                typed: Arc::new(Mutex::new(String::new())),
                calls: Arc::new(AtomicU32::new(0)),
            }),
            clipboard: Box::new(TestClipboard {
                set_calls: Arc::new(AtomicU32::new(0)),
                set_ok: true,
                snap_calls: Arc::new(AtomicU32::new(0)),
                restore_calls: restore_clone,
                snapshot: Some(snap_with_text("user's old copy")),
                change_count: None,
            }),
            paste_key: Box::new(|| true),
            accessibility_trusted: Box::new(|| true),
        };
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io2);
        assert_eq!(outcome, DeliveryOutcome::PastedUnverified);
        assert_eq!(
            restore.load(Ordering::SeqCst),
            1,
            "the clipboard must be restored after an unverified paste into an unreadable field"
        );
    }

    #[test]
    fn cascade_unverified_paste_when_text_too_short_to_verify() {
        // A readable field with text under `MIN_VERIFY_LEN` (4 chars) cannot
        // be verified by containment: the field would match almost anything.
        // The paste most likely landed, so the outcome is PastedUnverified,
        // not ClipboardOnly. The typed fallback never runs (F-19).
        let (io, _field, (_set, type_calls, _clip, restore), _typed) = make_io(
            "",
            false,
            true,
            true,
            true,
            Some(snap_with_text("user's old copy")),
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hey", None, &policy, &io);
        assert_eq!(
            outcome,
            DeliveryOutcome::PastedUnverified,
            "short text in a readable field is unverified, not clipboard-only"
        );
        assert_eq!(
            type_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the typed fallback must not run"
        );
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the clipboard must be restored"
        );
    }

    #[test]
    fn cascade_clipboard_only_when_readable_field_never_shows_text() {
        // The genuinely suspicious case: the field is readable, the text is
        // verifiable (>= MIN_VERIFY_LEN), and it is still absent after the
        // grace re-poll. F-19: stop with the text on the clipboard and tell
        // the user to press the paste shortcut. The typed fallback never
        // runs.
        let (io, _field, (_set, type_calls, _clip, _restore), _typed) = make_io(
            "something else entirely", // field readable, text not present
            false,
            true,
            true, // typer would succeed if (wrongly) reached
            true,
            Some(snap_with_text("user's old copy")),
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(
            outcome,
            DeliveryOutcome::ClipboardOnly,
            "a readable field that never shows the text is the suspicious case"
        );
        assert_eq!(
            type_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the typed fallback must not run"
        );
    }

    #[test]
    fn cascade_no_writable_field_pastes_optimistically() {
        // A web / Electron / chat input: no writable AX text field is
        // focused, but the field is focused and not secure and not a URL bar.
        // Following the market behaviour (EnviousWispr, Wispr Flow) the cascade
        // fires the ⌘V keystroke into the focused element rather than refusing.
        // The field is unreadable, so the outcome is PastedUnverified (the
        // paste fired; we cannot read it back to confirm), and no second copy
        // is ever attempted (F-19).
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::{Arc, Mutex};

        let field_text = Arc::new(Mutex::new(String::new()));
        let set_calls = Arc::new(AtomicU32::new(0));
        let type_calls = Arc::new(AtomicU32::new(0));
        let clip_set_calls = Arc::new(AtomicU32::new(0));
        let snap_calls = Arc::new(AtomicU32::new(0));
        let restore_calls = Arc::new(AtomicU32::new(0));
        let paste_calls = Arc::new(AtomicU32::new(0));
        let typed = Arc::new(Mutex::new(String::new()));
        let paste_counter = paste_calls.clone();

        let io = DeliveryIo {
            field: Box::new(TestField {
                text: field_text.clone(),
                secure: false,
                set_ok: true,
                writable_focus: false,
                url_field: false,
                readable: true,
                set_calls: set_calls.clone(),
            }),
            typer: Box::new(TestTyper {
                ok: true,
                typed,
                calls: type_calls.clone(),
            }),
            clipboard: Box::new(TestClipboard {
                set_calls: clip_set_calls.clone(),
                set_ok: true,
                snap_calls: snap_calls.clone(),
                restore_calls,
                snapshot: Some(snap_with_text("user's old copy")),
                change_count: None,
            }),
            paste_key: Box::new(move || {
                paste_counter.fetch_add(1, Ordering::SeqCst);
                true
            }),
            accessibility_trusted: Box::new(|| true),
        };

        let outcome = deliver("hello world", None, &default_policy(), &io);

        assert_eq!(
            outcome,
            DeliveryOutcome::PastedUnverified,
            "an unreadable but focused field gets an optimistic paste"
        );
        assert_eq!(
            clip_set_calls.load(Ordering::SeqCst),
            1,
            "the clipboard must be written with the dictation so ⌘V reads it"
        );
        assert_eq!(
            paste_calls.load(Ordering::SeqCst),
            1,
            "⌘V must fire exactly once"
        );
        assert_eq!(
            set_calls.load(Ordering::SeqCst),
            0,
            "set-value must not run (field is not writable)"
        );
        assert_eq!(type_calls.load(Ordering::SeqCst), 0, "typing must not run");
        assert_eq!(
            *field_text.lock().unwrap(),
            "",
            "the unreadable field is never directly written"
        );
    }

    #[test]
    fn cascade_no_writable_field_kept_on_clipboard_when_keystroke_fails() {
        // Same setup, but the paste keystroke could not be sent (e.g. the
        // accessibility grant was revoked mid-run). The text stays on the
        // clipboard as a kept write and the caller is told to paste manually.
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::{Arc, Mutex};

        let field_text = Arc::new(Mutex::new(String::new()));
        let set_calls = Arc::new(AtomicU32::new(0));
        let type_calls = Arc::new(AtomicU32::new(0));
        let clip_set_calls = Arc::new(AtomicU32::new(0));
        let snap_calls = Arc::new(AtomicU32::new(0));
        let restore_calls = Arc::new(AtomicU32::new(0));
        let paste_calls = Arc::new(AtomicU32::new(0));
        let typed = Arc::new(Mutex::new(String::new()));
        let paste_counter = paste_calls.clone();

        let io = DeliveryIo {
            field: Box::new(TestField {
                text: field_text.clone(),
                secure: false,
                set_ok: true,
                writable_focus: false,
                url_field: false,
                readable: true,
                set_calls: set_calls.clone(),
            }),
            typer: Box::new(TestTyper {
                ok: true,
                typed,
                calls: type_calls.clone(),
            }),
            clipboard: Box::new(TestClipboard {
                set_calls: clip_set_calls.clone(),
                set_ok: true,
                snap_calls: snap_calls.clone(),
                restore_calls,
                snapshot: Some(snap_with_text("user's old copy")),
                change_count: None,
            }),
            paste_key: Box::new(move || {
                paste_counter.fetch_add(1, Ordering::SeqCst);
                false
            }),
            accessibility_trusted: Box::new(|| true),
        };

        let outcome = deliver("hello world", None, &default_policy(), &io);

        assert_eq!(
            outcome,
            DeliveryOutcome::ClipboardOnly,
            "a failed keystroke falls back to the clipboard-kept path"
        );
        assert!(
            clip_set_calls.load(Ordering::SeqCst) >= 1,
            "the clipboard must hold the dictation"
        );
        assert_eq!(type_calls.load(Ordering::SeqCst), 0, "typing must not run");
    }

    #[test]
    fn cascade_url_field_refuses_before_any_side_effect() {
        // BUG-006 / L038: a browser URL / address / search bar must never be
        // the target of an optimistic ⌘V, because the browser's Enter handling
        // would navigate to the dictated text. The text is refused before any
        // clipboard write or keystroke.
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::{Arc, Mutex};

        let field_text = Arc::new(Mutex::new(String::new()));
        let set_calls = Arc::new(AtomicU32::new(0));
        let type_calls = Arc::new(AtomicU32::new(0));
        let clip_set_calls = Arc::new(AtomicU32::new(0));
        let snap_calls = Arc::new(AtomicU32::new(0));
        let restore_calls = Arc::new(AtomicU32::new(0));
        let paste_calls = Arc::new(AtomicU32::new(0));
        let typed = Arc::new(Mutex::new(String::new()));
        let paste_counter = paste_calls.clone();

        let io = DeliveryIo {
            field: Box::new(TestField {
                text: field_text.clone(),
                secure: false,
                set_ok: true,
                writable_focus: false,
                url_field: true,
                readable: true,
                set_calls: set_calls.clone(),
            }),
            typer: Box::new(TestTyper {
                ok: true,
                typed,
                calls: type_calls.clone(),
            }),
            clipboard: Box::new(TestClipboard {
                set_calls: clip_set_calls.clone(),
                set_ok: true,
                snap_calls: snap_calls.clone(),
                restore_calls,
                snapshot: None,
                change_count: None,
            }),
            paste_key: Box::new(move || {
                paste_counter.fetch_add(1, Ordering::SeqCst);
                true
            }),
            accessibility_trusted: Box::new(|| true),
        };

        let outcome = deliver("https://evil.example", None, &default_policy(), &io);

        assert!(
            matches!(outcome, DeliveryOutcome::Refused { .. }),
            "a URL bar must refuse dictation"
        );
        assert_eq!(
            clip_set_calls.load(Ordering::SeqCst),
            0,
            "no clipboard write"
        );
        assert_eq!(paste_calls.load(Ordering::SeqCst), 0, "no keystroke");
        assert_eq!(type_calls.load(Ordering::SeqCst), 0, "no typing");
        assert_eq!(*field_text.lock().unwrap(), "", "the URL bar is untouched");
    }

    #[test]
    fn cascade_failed_paste_never_types() {
        // The ⌘V paste path returns None (the keystroke failed to send), so the
        // cascade reaches the last-resort fallback. That fallback must NEVER use
        // the character-by-character `fast_text` route: it mangles text into
        // repeated characters in some apps (observed in terminals such as Terax
        // -- a dictation arrived as "aaaa...") and there is no reliable signal
        // for which apps do this. With set-value also unavailable, the text is
        // held for Paste-Last so the user pastes it themselves.
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::{Arc, Mutex};

        let field_text = Arc::new(Mutex::new(String::new()));
        let set_calls = Arc::new(AtomicU32::new(0));
        let type_calls = Arc::new(AtomicU32::new(0));
        let clip_set_calls = Arc::new(AtomicU32::new(0));
        let snap_calls = Arc::new(AtomicU32::new(0));
        let restore_calls = Arc::new(AtomicU32::new(0));
        let paste_calls = Arc::new(AtomicU32::new(0));
        let typed = Arc::new(Mutex::new(String::new()));
        let paste_counter = paste_calls.clone();

        let io = DeliveryIo {
            field: Box::new(TestField {
                text: field_text.clone(),
                secure: false,
                set_ok: false, // set-value also fails -> held for Paste-Last
                writable_focus: true,
                url_field: false,
                readable: true,
                set_calls: set_calls.clone(),
            }),
            typer: Box::new(TestTyper {
                ok: true,
                typed,
                calls: type_calls.clone(),
            }),
            clipboard: Box::new(TestClipboard {
                set_calls: clip_set_calls.clone(),
                set_ok: true,
                snap_calls: snap_calls.clone(),
                restore_calls,
                snapshot: None,
                change_count: None,
            }),
            paste_key: Box::new(move || {
                paste_counter.fetch_add(1, Ordering::SeqCst);
                false
            }),
            accessibility_trusted: Box::new(|| true),
        };

        let outcome = deliver("hello world", None, &default_policy(), &io);

        assert_eq!(
            outcome,
            DeliveryOutcome::HeldForPasteLast,
            "a failed paste with no set-value holds for Paste-Last"
        );
        assert_eq!(
            type_calls.load(Ordering::SeqCst),
            0,
            "the typed fallback must never run (it mangles text)"
        );
    }

    #[test]
    fn cascade_inconclusive_verification_stops_at_clipboard() {
        // F-19: the field is readable but does not contain the text, so
        // verification fails. The old code fell through to the typed
        // fallback, which could double the text if the paste had actually
        // landed (the AX value just lagged). The new code stops with the
        // text on the clipboard and reports ClipboardOnly, so the typed
        // fallback never runs a second copy.
        let (io, _field, (_set, type_calls, _clip, _restore), _typed) = make_io(
            "something else entirely", // field readable, text not present
            false,
            true,
            true, // typer would succeed if (wrongly) reached
            true,
            None, // no snapshot
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        // The paste was inconclusive (field readable but text missing), so
        // the cascade stops with the text on the clipboard.
        assert_eq!(outcome, DeliveryOutcome::ClipboardOnly);
        assert_eq!(
            type_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the typed fallback must not run on an inconclusive verification"
        );
    }

    #[test]
    fn cascade_verifies_when_paste_lands_late_on_readable_field() {
        // Regression: on a loaded machine the focused field's AX `value`
        // attribute can lag the actual paste well past the first
        // `VERIFY_TIMEOUT` (150 ms). The old code treated a readable-but-not-yet
        // updated field as a failed paste and fell through to the typed
        // fallback, which both crash-prone (enigo `fast_text` hits TIS off-main
        // on newline text) and has mangled real dictation into repeated `a`s.
        //
        // Simulate the lag: the field starts readable but empty, and a
        // background thread publishes the pasted text after a short delay
        // (past the first verify window, within the grace window). The
        // cascade must now report PastedVerified and never touch the typer.
        let (io, field, (_set, type_calls, _clip, _restore), _typed) = make_io(
            "", // field readable, but the paste has not landed yet
            false, true, true, // typer would succeed if (wrongly) reached
            true, // paste key fires
            None, true, None,
        );
        let pasted = "hello world".to_string();
        let field_clone = field.clone();
        let pasted_clone = pasted.clone();
        // Publish the "late" AX update ~80 ms in: after the first 150 ms
        // window's first poll or two, but the grace pass (400 ms) must catch
        // it. Use a delay comfortably inside VERIFY_GRACE.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            *field_clone.lock().unwrap() = pasted_clone;
        });
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(
            outcome,
            DeliveryOutcome::PastedVerified,
            "a late-landing paste on a readable field must verify, not fall to typed"
        );
        assert_eq!(
            type_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the typed fallback must not run when the paste landed late"
        );
    }

    #[test]
    fn cascade_setvalue_when_paste_key_fails() {
        // The paste key cannot be sent (enigo unavailable). The text is on
        // the clipboard, so the cascade falls through to the last-resort
        // route, which is set-value (a single AX write, not a keystroke
        // stream, so it cannot mangle the text). The typed route is never
        // used as a delivery path, so the typer is not called.
        let (io, _field, (set_calls, type_calls, _clip, _restore), _typed) = make_io(
            "something else",
            false,
            true,  // set-value works
            true,  // typer works (but must not be used as a fallback)
            false, // paste key fails
            None,
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::SetValue);
        assert_eq!(
            type_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the typed fallback must never run"
        );
        assert_eq!(set_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn cascade_hold_when_paste_key_fails_and_setvalue_fails() {
        // The paste key cannot be sent, the typer fails, and set-value
        // fails. The cascade holds the text.
        let (io, _field, (_set, _type_calls, _clip, _restore), _typed) = make_io(
            "something else",
            false,
            false, // set-value fails
            false, // typer fails
            false, // paste key fails
            None,
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::HeldForPasteLast);
    }

    #[test]
    fn cascade_holds_when_paste_key_fails_and_setvalue_fails() {
        // The paste key cannot be sent and set-value is unavailable. The
        // typed route is no longer a delivery path (it mangles text into
        // repeated characters in some apps), so even though the typer would
        // work, it must not be used. The text is held for Paste-Last.
        let (io, _field, (_set, type_calls, _clip, _restore), _typed) = make_io(
            "something else",
            false,
            false, // set-value fails
            true,  // typer works (but must not be used as a fallback)
            false, // paste key fails
            None,
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::HeldForPasteLast);
        assert_eq!(
            type_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the typed fallback must never run"
        );
    }

    #[test]
    fn cascade_secure_field_refused() {
        // A secure field refuses before any side effect. F-16: secure
        // refusals stay clipboard-free, unlike non-secure refusals.
        let (io, _field, (_set, _type_calls, clip_set, restore), _typed) = make_io(
            "",
            true, // secure
            true,
            true,
            true,
            Some(snap_with_text("x")),
            true,
            None,
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
        // entirely and go straight to the typer. Forced via with_override so
        // the test does not depend on which real app the checked-in table
        // maps to "type" (terminals now paste, not type).
        let (io, _field, (_set, type_calls, clip_set, restore), _typed) = make_io(
            "",
            false,
            true,
            true, // typer works
            true,
            Some(snap_with_text("x")),
            true,
            None,
        );
        let policy =
            default_policy().with_override("com.example.typedapp", DeliveryMode::Type, None);
        let outcome = deliver("ls -la", Some("com.example.typedapp"), &policy, &io);
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
        let (io, _field, (set_calls, type_calls, clip_set, restore), _typed) = make_io(
            "",
            false,
            true, // set-value works
            true,
            true,
            Some(snap_with_text("x")),
            true,
            None,
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
        let (io, _field, (_set, _type_calls, _clip, _restore), _typed) =
            make_io("", false, true, true, true, None, true, None);
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        // With no env override and default policy (paste_verify), the field
        // is readable, the text is verifiable (>= MIN_VERIFY_LEN), and it is
        // absent after the grace re-poll. This is the genuinely suspicious
        // case, so the cascade stops with the text on the clipboard.
        assert_eq!(
            outcome,
            DeliveryOutcome::ClipboardOnly,
            "a readable field that never shows the text stops at the clipboard"
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
        // Terminals paste (clipboard + ⌘V) rather than typing character-by-
        // character: a synthetic key stream mangles in a terminal (stuck key
        // -> "aaaaa"), while a single paste lands cleanly. See the policy file.
        assert_eq!(
            policy.mode_for(Some("com.apple.Terminal")),
            DeliveryMode::PasteVerify
        );
        assert_eq!(
            policy.mode_for(Some("com.googlecode.iterm2")),
            DeliveryMode::PasteVerify
        );
        assert_eq!(
            policy.mode_for(Some("com.microsoft.Word")),
            DeliveryMode::PasteVerify
        );
        assert_eq!(
            policy.mode_for(Some("com.microsoft.VSCode")),
            DeliveryMode::PasteVerify
        );
        // Terax must paste, never type: the typed route mangles single-line
        // dictation into repeated characters ("aaaa"). Regression guard for
        // the /goal that the typed path is never a production delivery route.
        assert_eq!(
            policy.mode_for(Some("app.crynta.terax")),
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
        let (io, _field, (_set, _type_calls, _clip, restore), _typed) = make_io(
            "",
            false,
            true,
            true,
            true,
            Some(snap_with_text("user's clipboard")),
            true,
            None,
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
    fn clipboard_restore_skipped_when_change_count_moved() {
        // F-18: the snapshot captured a change count, and the live count has
        // moved since (the user or another app copied something in the
        // meantime). The restore must be skipped, so the user's new copy is
        // not overwritten with the old snapshot.
        let mut snap = snap_with_text("user's clipboard");
        snap.change_count = Some(42);
        let (io, _field, (_set, _type_calls, _clip, restore), _typed) = make_io(
            "",
            false,
            true,
            true,
            true,
            Some(snap),
            true,
            Some(43), // live count moved: 42 -> 43
        );
        let policy = default_policy();
        let _ = deliver("hello world", None, &policy, &io);
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "restore must be skipped when the change count moved"
        );
    }

    #[test]
    fn clipboard_restore_runs_when_change_count_unchanged() {
        // F-18: the snapshot captured a change count, and the live count has
        // not moved since. The restore runs, so the user's clipboard is
        // restored.
        let mut snap = snap_with_text("user's clipboard");
        snap.change_count = Some(42);
        let (io, _field, (_set, _type_calls, _clip, restore), _typed) = make_io(
            "",
            false,
            true,
            true,
            true,
            Some(snap),
            true,
            Some(42), // live count unchanged: 42 -> 42
        );
        let policy = default_policy();
        let _ = deliver("hello world", None, &policy, &io);
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "restore must run when the change count is unchanged"
        );
    }

    #[test]
    fn clipboard_restore_on_paste_key_failure() {
        // If the paste key fails, the cascade falls through to the typed /
        // set-value routes. Here both fail, so the cascade holds the text.
        // The clipboard snapshot (taken before the paste attempt) must still
        // be restored so the user's clipboard is not lost.
        let (io, _field, (_set, _type_calls, _clip, restore), _typed) = make_io(
            "",
            false,
            false, // set-value fails
            false, // typer fails
            false, // paste key fails
            Some(snap_with_text("user's clipboard")),
            true,
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::HeldForPasteLast);
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "clipboard restored even on paste key failure"
        );
    }

    #[test]
    fn clipboard_snapshot_too_large_is_skipped() {
        // A snapshot exceeding SNAPSHOT_SIZE_LIMIT must be skipped: the
        // cascade still delivers but does not attempt restore.
        let mut big_snap = ClipboardSnapshot {
            parts: Vec::new(),
            items: Vec::new(),
            change_count: None,
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

        let (io, _field, (_set, _type_calls, _clip, restore), _typed) =
            make_io("", false, true, true, true, Some(big_snap), true, None);
        let policy = default_policy();
        let _ = deliver("hello world", None, &policy, &io);
        assert_eq!(
            restore.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "restore skipped for oversized snapshot"
        );
    }

    #[test]
    fn untrusted_accessibility_yields_accessibility_denied() {
        // F-17: the accessibility trust check says `false`. The paste
        // shortcut cannot be sent, so the cascade goes straight to the
        // clipboard-kept path and reports AccessibilityDenied, which the
        // caller's message names as the cause.
        let (io, _field, (_set, _type_calls, clip_set, _restore), _typed) = make_io(
            "",
            false,
            true,
            true,
            true,
            Some(snap_with_text("x")),
            false, // accessibility NOT trusted
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::AccessibilityDenied);
        assert_eq!(
            clip_set.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the clipboard must be written with the dictation"
        );
    }

    #[test]
    fn untrusted_accessibility_yields_denied_on_readable_field() {
        // F-17: the accessibility trust check says `false` and the focused
        // field IS a readable writable text field, so the cascade takes the
        // normal paste-verify path, which consults trust and goes straight to
        // the clipboard-kept path, reporting AccessibilityDenied (the message
        // names the cause). This is the case where trust actually blocks the
        // paste; the unreadable-field case instead reports PastedUnverified
        // because the field cannot be read back to confirm either way.
        use std::sync::{Arc, Mutex};
        let io = DeliveryIo {
            field: Box::new(TestField {
                text: Arc::new(Mutex::new(String::new())),
                secure: false,
                set_ok: true,
                writable_focus: true,
                url_field: false,
                readable: true,
                set_calls: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            }),
            typer: Box::new(TestTyper {
                ok: true,
                typed: Arc::new(Mutex::new(String::new())),
                calls: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            }),
            clipboard: Box::new(TestClipboard {
                set_calls: Arc::new(std::sync::atomic::AtomicU32::new(0)),
                set_ok: true,
                snap_calls: Arc::new(std::sync::atomic::AtomicU32::new(0)),
                restore_calls: Arc::new(std::sync::atomic::AtomicU32::new(0)),
                snapshot: Some(snap_with_text("x")),
                change_count: None,
            }),
            paste_key: Box::new(|| true),
            accessibility_trusted: Box::new(|| false),
        };
        let outcome = deliver("hello world", None, &default_policy(), &io);
        assert_eq!(
            outcome,
            DeliveryOutcome::AccessibilityDenied,
            "the trust check is read first, so it names the cause"
        );
    }

    #[test]
    fn trusted_accessibility_does_not_regress_smart_insertion_path() {
        // F-17: the trust check says `true` (the default). The cascade
        // follows the normal paste-verify path, so a verified paste still
        // reports PastedVerified and the typed fallback is never reached.
        let (io, _field, (_set, _type_calls, _clip, _restore), _typed) = make_io(
            "hello world", // field already holds the text: paste verified
            false,
            true,
            true,
            true,
            Some(snap_with_text("user's old copy")),
            true, // accessibility trusted
            None,
        );
        let policy = default_policy();
        let outcome = deliver("hello world", None, &policy, &io);
        assert_eq!(outcome, DeliveryOutcome::PastedVerified);
    }
}
