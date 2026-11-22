//! Remote crash and hang reporting (Sentry).
//!
//! ## What is sent
//!
//! Only three things ever leave the machine, and none of them is user content:
//!
//! 1. A **panic**: its message (a Rust payload the developer wrote), the
//!    location, and a stack trace.
//! 2. A **hang**: how long the main thread was unresponsive and how many
//!    watchdog probes went unserved, plus the watchdog thread's stack, which
//!    usually names whatever the main thread was blocked in.
//! 3. The app version and the OS.
//!
//! Never sent: dictated text, transcripts, clipboard content, screen text,
//! audio, the log ring, file paths from the user's home directory, or the
//! Sentry "user" object (no account, no IP-based identity:
//! `send_default_pii(false)`, and the scrubber in [`scrub`] deletes the
//! `user`, `request`, and `server_name` fields outright if a future
//! integration ever sets them).
//!
//! ## Consent
//!
//! [`ENABLED`] mirrors `Settings::send_crash_reports`, on by default and
//! switched off in Settings → Privacy. It starts `false` so nothing can be
//! sent before the settings file has been read; `before_send` re-checks it at
//! send time, which also covers a panic that happens seconds after the user
//! turned the toggle off. `TELETYPE_TELEMETRY=off` is the hard kill switch for
//! a developer machine, and `TELETYPE_SENTRY_DSN` overrides the endpoint
//! (empty disables reporting entirely, which the e2e harness uses).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use sentry::protocol::{Context, Event, Level};
use sentry::ClientInitGuard;

/// The project DSN (`duas-and-aamal/teletype`). A Sentry DSN is a public,
/// write-only routing key that ships inside every client binary by design; it
/// is not a credential and cannot read data back. `TELETYPE_SENTRY_DSN`
/// overrides it at runtime.
pub const CLIENT_DSN: &str = "https://7921ce706a8c1b7fe4b32ed49105a65c@o4510837236695040.ingest.de.sentry.io/4512476715090000";

/// Whether the user has consented to remote crash reports. Set from the
/// settings file once it has loaded, and again on every settings change.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether [`init`] was asked to start a client at all. When it was not (e2e
/// runs, a build without a DSN), [`enabled`] is always false and the capture
/// helpers are no-ops even if a stale settings file says "on".
static STARTED: AtomicBool = AtomicBool::new(false);

/// When the client started, so every event can carry how long the app had been
/// running. A hang at 30 s is a startup bug; at 6 h it is a leak.
static STARTED_AT: OnceLock<Instant> = OnceLock::new();

/// Set while the panic hook writes its own lines. Those lines are breadcrumbs
/// on the panic event that the SDK is already about to send; without this,
/// each one ("PANIC: …", "  at src/…", "  thread: main") became its own
/// Sentry issue, so one crash arrived as three.
static MUTE_ERROR_EVENTS: AtomicBool = AtomicBool::new(false);

/// Starts the Sentry client and returns the guard that flushes pending events
/// on drop. The caller keeps the guard alive for the whole process.
///
/// Called from [`crate::run`] before the panic hook and before any thread is
/// spawned, so every thread inherits the client.
pub fn init() -> Option<ClientInitGuard> {
    let dsn = resolve_dsn(
        std::env::var("TELETYPE_SENTRY_DSN").ok(),
        std::env::var("TELETYPE_TELEMETRY")
            .map(|v| v.eq_ignore_ascii_case("off"))
            .unwrap_or(false),
        CLIENT_DSN,
    )?;
    let guard = sentry::init(
        sentry::ClientOptions::new()
            .dsn(&dsn)
            .release(release())
            .environment(environment())
            // No account, no IP, no cookies: this app has no users table.
            .send_default_pii(false)
            // A `capture_message` (a hang) is only useful with a stack, and
            // the watchdog's stack names the code the main thread was in.
            .attach_stacktrace(true)
            // Breadcrumbs would be log lines, and this app's log lines can
            // mention dictated text. Send the event alone.
            .max_breadcrumbs(50)
            .before_send(finalize),
    );
    let _ = STARTED_AT.set(Instant::now());
    STARTED.store(true, Ordering::Relaxed);
    Some(guard)
}

/// Records the user's consent, from the settings file at startup and from the
/// Settings toggle afterwards.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether an event would be sent right now: a client was started and the
/// user consents.
pub fn enabled() -> bool {
    STARTED.load(Ordering::Relaxed) && ENABLED.load(Ordering::Relaxed)
}

/// Records one application log line for the next event: a breadcrumb, plus a
/// standalone event for an error-level line so a failure that never panics is
/// still visible in Sentry.
///
/// The panic path is excluded because the panic integration already reports
/// that panic with a real stack trace; `"PANIC"` and `"  at "` are the two
/// lines [`crate::run`]'s hook writes.
pub fn record_log(level: &str, message: &str) {
    if !enabled() {
        return;
    }
    let breadcrumb_level = match level {
        "error" => Level::Error,
        "warn" => Level::Warning,
        _ => Level::Info,
    };
    sentry::add_breadcrumb(sentry::Breadcrumb {
        level: breadcrumb_level,
        category: Some("app-log".into()),
        message: Some(truncate(message, 300)),
        ..Default::default()
    });
    if breadcrumb_level == Level::Error
        && !message.starts_with("PANIC")
        && !MUTE_ERROR_EVENTS.load(Ordering::Relaxed)
    {
        sentry::with_scope(
            |scope| {
                scope.set_tag("source", "app-log");
                scope.set_tag("failure.kind", "error-log");
            },
            || sentry::capture_message(&truncate(message, 300), Level::Error),
        );
    }
}

/// Stops error-level log lines from becoming their own events, while still
/// recording them as breadcrumbs. Called by the panic hook, which is already
/// reporting that panic through the SDK's panic integration.
pub fn mute_error_events() {
    MUTE_ERROR_EVENTS.store(true, Ordering::Relaxed);
}

/// Reports a main-thread freeze. Called from the watchdog thread, never the
/// main thread; the SDK's transport runs on its own thread, so this returns
/// without waiting on the network even while the main thread is wedged.
pub fn capture_hang(unresponsive_ms: u64, missed_probes: u32) {
    if !enabled() {
        return;
    }
    sentry::with_scope(
        |scope| {
            scope.set_tag("failure.kind", "hang");
            scope.set_tag("detector", "main-thread-watchdog");
            scope.set_extra("unresponsiveMs", unresponsive_ms.into());
            scope.set_extra("missedProbes", missed_probes.into());
        },
        || {
            sentry::capture_message(
                &format!(
                    "Teletype main thread unresponsive for {:.1}s",
                    unresponsive_ms as f64 / 1000.0
                ),
                Level::Warning,
            );
        },
    );
}

/// The DSN to use, or `None` for a build that reports nothing. Pure so the
/// env seams are testable without touching the process environment.
pub(crate) fn resolve_dsn(
    env_dsn: Option<String>,
    telemetry_off: bool,
    built_in: &str,
) -> Option<String> {
    if telemetry_off {
        return None;
    }
    let dsn = match env_dsn {
        Some(v) => v,
        None => built_in.to_string(),
    };
    let dsn = dsn.trim();
    (!dsn.is_empty()).then(|| dsn.to_string())
}

/// `teletype@0.2.22`, matching the release name CI uses, so an issue links to
/// the shipped build.
fn release() -> String {
    format!("teletype@{}", env!("CARGO_PKG_VERSION"))
}

/// Release builds report as `production`, a developer build as `development`,
/// so local test errors never look like field crashes.
fn environment() -> String {
    if cfg!(debug_assertions) {
        "development".into()
    } else {
        "production".into()
    }
}

/// Last stop before an event goes out: drops it if consent is gone, attaches
/// the device/app context, and strips any field that could identify the user.
fn finalize(mut event: Event<'static>) -> Option<Event<'static>> {
    if !enabled() {
        return None;
    }
    event.user = None;
    event.request = None;
    event.server_name = None;
    // The app's own working directory can contain a user name (`/Users/ali`).
    event.extra.remove("process_path");
    // One place for device information, so every event answers "what was this
    // running on?" without the caller repeating it. The `contexts`
    // integration adds OS name and version, CPU and memory; this adds the
    // app-side facts that it cannot know.
    let mut app = BTreeMap::new();
    app.insert("version".to_string(), env!("CARGO_PKG_VERSION").into());
    app.insert("release".to_string(), release().into());
    app.insert("environment".to_string(), environment().into());
    app.insert("arch".to_string(), std::env::consts::ARCH.into());
    app.insert("os".to_string(), std::env::consts::OS.into());
    app.insert("locale".to_string(), locale().into());
    app.insert(
        "cores".to_string(),
        std::thread::available_parallelism()
            .map(|n| n.get() as u64)
            .unwrap_or(0)
            .into(),
    );
    if let Some(started) = STARTED_AT.get() {
        app.insert("uptimeSecs".to_string(), started.elapsed().as_secs().into());
    }
    event
        .contexts
        .insert("app".to_string(), Context::Other(app));
    Some(event)
}

/// The user's locale, for reproducing formatting and keyboard behaviour. Not
/// the language they dictated in: that is content.
fn locale() -> String {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return value;
            }
        }
    }
    "unknown".into()
}

/// Bounds what one log line can cost on the wire. Log messages are written by
/// developers, but a formatted one can interpolate a long value.
fn truncate(message: &str, max_chars: usize) -> String {
    if message.chars().count() <= max_chars {
        return message.to_string();
    }
    let mut out: String = message.chars().take(max_chars).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_built_in_dsn_is_used_when_nothing_overrides_it() {
        assert_eq!(
            resolve_dsn(None, false, "https://key@o1.ingest.sentry.io/1").as_deref(),
            Some("https://key@o1.ingest.sentry.io/1")
        );
    }

    #[test]
    fn the_env_override_wins() {
        assert_eq!(
            resolve_dsn(
                Some(" https://other@o9.ingest.sentry.io/9 ".into()),
                false,
                "built-in"
            )
            .as_deref(),
            Some("https://other@o9.ingest.sentry.io/9")
        );
    }

    #[test]
    fn an_empty_override_or_the_kill_switch_disables_reporting() {
        assert_eq!(resolve_dsn(Some(String::new()), false, "built-in"), None);
        assert_eq!(resolve_dsn(Some("   ".into()), false, "built-in"), None);
        assert_eq!(resolve_dsn(None, true, "https://key@o1/1"), None);
    }

    #[test]
    fn nothing_is_sent_without_consent() {
        // No client was started in a unit test, so even "enabled" consents
        // cannot produce an event.
        set_enabled(true);
        assert!(!enabled());
        assert!(finalize(Event::default()).is_none());
    }

    #[test]
    fn a_long_log_line_is_bounded() {
        let long = "x".repeat(4000);
        let got = truncate(&long, 300);
        assert_eq!(got.chars().count(), 301);
        assert!(got.ends_with('…'));
        assert_eq!(truncate("short", 300), "short");
        // Multi-byte characters are counted as characters, not bytes.
        assert_eq!(truncate("éé", 1), "é…");
    }
}
