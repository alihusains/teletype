//! Closed-set placeholder expansion for AutoText replacement values.
//!
//! A replacement may contain `{{date}}`, `{{time}}`, `{{clipboard}}` or
//! `{{app}}`; each occurrence is expanded **at expansion time** (every use
//! gets a fresh value), never at save time. Unknown `{{...}}` tokens are left
//! verbatim. Expansion never fails: an unreadable clipboard or unknown app
//! becomes an empty string.
//!
//! The date/time math is dependency-free (same civil-date algorithm as
//! [`crate::history`]); the local timezone offset is probed at call time so
//! DST transitions are honored.

use std::time::SystemTime;

use crate::context::ApplicationContext;

/// Expands the closed set of placeholders in `text`.
///
/// - `{{date}}` → local date, `YYYY-MM-DD` (the default format)
/// - `{{time}}` → local time, `HH:MM` (24h)
/// - `{{clipboard}}` → current clipboard text (empty string on error)
///
/// `{{app}}` needs an application context; use [`expand_placeholders_for`] on
/// paths where the frontmost app is known. Here it is preserved verbatim.
/// Any other `{{...}}` is preserved exactly as written.
pub fn expand_placeholders(text: &str) -> String {
    expand_placeholders_with(text, None, "", &read_clipboard)
}

/// Like [`expand_placeholders`], but also resolves `{{app}}` to the name of
/// the frontmost application: its id when the name is empty, and an empty
/// string when the app is unknown.
pub fn expand_placeholders_for(text: &str, app: &ApplicationContext) -> String {
    expand_placeholders_with(text, Some(app), "", &read_clipboard)
}

/// Like [`expand_placeholders_for`], but `{{date}}` is rendered in the
/// user's chosen format. `date_format` is a named id from
/// [`date_format_template`] (e.g. `"us"`) or a raw template for the free-text
/// custom option; `""` (or `"iso"`) is the default `YYYY-MM-DD`.
pub fn expand_placeholders_with(
    text: &str,
    app: Option<&ApplicationContext>,
    date_format: &str,
    clipboard: &dyn Fn() -> String,
) -> String {
    if !text.contains("{{") {
        return text.to_string();
    }
    // Probe the clock and the clipboard only for tokens that are present:
    // a template that only uses `{{app}}` must not read the user's clipboard.
    let (date, time) = if text.contains("{{date}}") || text.contains("{{time}}") {
        let local = local_now_secs();
        let days = local.div_euclid(86_400);
        let rem = local.rem_euclid(86_400);
        let template = date_format_template(date_format).unwrap_or(date_format);
        let date = render_date_template(template, days * 86_400);
        let time = format!("{:02}:{:02}", rem / 3600, (rem % 3600) / 60);
        (date, time)
    } else {
        (String::new(), String::new())
    };
    let clip = if text.contains("{{clipboard}}") {
        clipboard()
    } else {
        String::new()
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        // Copy everything up to the token start.
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        match after_open.find("}}") {
            Some(end) => {
                let name = &after_open[..end];
                match name {
                    "date" => out.push_str(&date),
                    "time" => out.push_str(&time),
                    "clipboard" => out.push_str(&clip),
                    "app" => match app {
                        Some(app) => out.push_str(app_display_name(app)),
                        None => out.push_str("{{app}}"),
                    },
                    _ => {
                        // Unknown token: keep it verbatim.
                        out.push_str("{{");
                        out.push_str(name);
                        out.push_str("}}");
                    }
                }
                rest = &after_open[end + 2..];
            }
            None => {
                // No closing braces: keep the rest verbatim.
                out.push_str(&rest[start..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The name to show for the frontmost app: trimmed display name, else id.
fn app_display_name(app: &ApplicationContext) -> &str {
    let name = app.application_name.trim();
    if name.is_empty() {
        app.application_id.trim()
    } else {
        name
    }
}

/// Named date formats offered by the AutoText "Date format" dropdown.
///
/// Each id maps to a template rendered by [`render_date_template`]. The ids
/// are stable: a stored snippet may carry one, and the UI lists them.
/// An id that is not in this table is not an error: it is treated as a raw
/// template, which is how the free-text "Custom..." option works.
pub fn date_format_template(id: &str) -> Option<&'static str> {
    match id {
        "" | "iso" => Some("YYYY-MM-DD"),
        "us" => Some("MM/DD/YYYY"),
        "us-short" => Some("MM/DD/YY"),
        "eu" => Some("DD/MM/YYYY"),
        "eu-short" => Some("DD/MM/YY"),
        "long" => Some("D MMM YYYY"),
        "long-us" => Some("MMMM D, YYYY"),
        "dmy-dash" => Some("DD-MM-YYYY"),
        "ymd-slash" => Some("YYYY/MM/DD"),
        "mdy-dash" => Some("MM-DD-YYYY"),
        "dmy-dash-short" => Some("DD-MM-YY"),
        _ => None,
    }
}

/// Renders `template` against `now` (local Unix seconds).
///
/// Tokens, longest first: `YYYY` (four-digit year), `YY` (two-digit year),
/// `MMMM` (full month name, e.g. `November`), `MMM` (abbreviated, e.g.
/// `Nov`), `MM` (zero-padded month), `M` (month), `DD` (zero-padded day),
/// `D` (day). Anything else, including literal separators like `-` and `/`,
/// is copied through. A template with no recognized tokens is returned
/// verbatim, so a user's free-text custom template never fails.
pub fn render_date_template(template: &str, now: i64) -> String {
    let (y, m, d) = civil_from_days(now.div_euclid(86_400));
    let months_abbr = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let months_full = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let y_str = y.to_string();
    let m_str = m.to_string();
    let d_str = d.to_string();
    let y2 = if y_str.len() >= 2 {
        &y_str[y_str.len() - 2..]
    } else {
        &y_str
    };
    let m2 = if m_str.len() < 2 {
        format!("0{m_str}")
    } else {
        m_str.clone()
    };
    let d2 = if d_str.len() < 2 {
        format!("0{d_str}")
    } else {
        d_str.clone()
    };
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some((start, token)) = next_date_token(rest) {
        out.push_str(&rest[..start]);
        match token {
            "YYYY" => out.push_str(&y_str),
            "YY" => out.push_str(y2),
            "Y" => out.push_str(&y_str),
            "MMMM" => out.push_str(months_full[(m - 1) as usize]),
            "MMM" => out.push_str(months_abbr[(m - 1) as usize]),
            "MM" => out.push_str(&m2),
            "DD" => out.push_str(&d2),
            "M" => out.push_str(&m_str),
            "D" => out.push_str(&d_str),
            _ => unreachable!("next_date_token only yields date tokens"),
        }
        rest = &rest[start + token.len()..];
    }
    out.push_str(rest);
    out
}

/// Finds the next date token in `s`: its (start, normalized text).
/// Longest match wins at a given position (`MMMM` before `MMM` before `M`),
/// and a run like `MMY` is not a token, so a later real token still wins.
fn next_date_token(s: &str) -> Option<(usize, &'static str)> {
    for (i, c) in s.char_indices() {
        if !matches!(c, 'Y' | 'M' | 'D') {
            continue;
        }
        // Longest-first at this position; every char of the run must match
        // the first, so `MMY` falls through to a shorter (or no) match.
        for len in [4usize, 3, 2, 1] {
            if let Some(run) = s.get(i..i + len) {
                if run.chars().all(|ch| ch == c) {
                    let token = match (c, len) {
                        ('Y', 4) => "YYYY",
                        ('Y', 2) => "YY",
                        ('Y', 1) => "Y",
                        ('M', 4) => "MMMM",
                        ('M', 3) => "MMM",
                        ('M', 2) => "MM",
                        ('M', 1) => "M",
                        ('D', 2) => "DD",
                        ('D', 1) => "D",
                        _ => continue,
                    };
                    return Some((i, token));
                }
            }
        }
    }
    None
}

/// Unix seconds of the local wall clock: UTC epoch + probed local offset.
fn local_now_secs() -> i64 {
    let utc = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    utc + local_offset_secs()
}

/// Seconds ahead of UTC for the local timezone, probed by reading the local
/// wall clock and the UTC epoch back-to-back. Falls back to 0 (UTC) when the
/// probe is impossible or yields an implausible offset.
fn local_offset_secs() -> i64 {
    let local = local_wall_secs();
    let utc = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let offset = local - utc;
    // The two reads can straddle a second boundary; anything beyond an hour
    // means the probe failed.
    if offset.abs() > 3600 {
        0
    } else {
        offset
    }
}

/// Local wall-clock seconds since the Unix epoch.
///
/// On Unix this is `time(NULL)` (which is the local wall clock, not UTC).
/// On non-Unix platforms we fall back to the UTC epoch (offset 0) — non-Unix
/// is not the primary target for this feature.
fn local_wall_secs() -> i64 {
    #[cfg(unix)]
    {
        extern "C" {
            fn time(sec: *mut i64) -> i64;
        }
        let mut t: i64 = 0;
        // SAFETY: `time` writes a single machine word to a pointer we own.
        let r = unsafe { time(&mut t) };
        // `time` returns -1 on error (never for a valid pointer); treat as 0.
        if r == -1 {
            0
        } else {
            t
        }
    }
    #[cfg(not(unix))]
    {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// Converts days-since-1970 to a (year, month, day) civil date.
/// (Same algorithm as [`crate::history::civil_from_days`].)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// True on a headless runner (GitHub Actions sets `CI=true`).
fn is_headless() -> bool {
    matches!(std::env::var("CI"), Ok(v) if !v.is_empty() && v != "false")
}

/// Public wrapper over [`read_clipboard`] for the expansion paths in
/// [`super::expand`]: they always want the real clipboard (a fake source is
/// only useful in unit tests, which call [`expand_placeholders_with`] directly).
pub fn read_clipboard_public() -> String {
    read_clipboard()
}

/// Reads the current clipboard text. On any error (clipboard daemon down,
/// no display, permission denied) returns an empty string and logs a
/// warning — expansion must never fail.
///
/// On a headless runner (CI) we return empty *without touching the real
/// clipboard at all*. The macOS clipboard is `NSPasteboard`, which needs the
/// window server; on a headless box the ObjC call raises a *foreign*
/// exception that `arboard` maps to a Rust `Err` but that stays pending in
/// the ObjC runtime. The next thread to enter ObjC then aborts the whole
/// process with "Rust cannot catch foreign exceptions" — an abort, not a
/// catchable error. Skipping the call entirely on CI is the only safe option.
fn read_clipboard() -> String {
    if is_headless() {
        return String::new();
    }
    // Serialised with every other pasteboard touch in the process: reading
    // `NSPasteboard` while another thread writes it crashes in
    // `objc_msgSend`, and a `{{clipboard}}` entry is exactly the kind of
    // incidental read that can land on top of a dictation's clipboard restore.
    let _pasteboard = crate::injector::pasteboard_lock();
    match arboard::Clipboard::new() {
        Ok(mut cb) => match cb.get_text() {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(
                    "[autotext] {{clipboard}} unreadable ({e}); expanding to empty string"
                );
                String::new()
            }
        },
        Err(e) => {
            tracing::warn!("[autotext] clipboard unavailable ({e}); expanding to empty string");
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The civil-date math is pinned to known dates.
    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    /// Inverts [`civil_from_days`]: (y, m, d) → days since 1970-01-01.
    fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = if m > 2 { m as i64 - 3 } else { m as i64 + 9 };
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// `{{date}}` and `{{time}}` produce the right *shape* and a value within
    /// 90 s of the real local clock (the clock is not frozen).
    #[test]
    fn date_and_time_expand_with_correct_shape() {
        let out = expand_placeholders("{{date}} {{time}}");
        let parts: Vec<&str> = out.split(' ').collect();
        assert_eq!(parts.len(), 2, "expected 'YYYY-MM-DD HH:MM', got '{out}'");
        let (date, time) = (parts[0], parts[1]);
        // Shape checks.
        assert_eq!(date.len(), 10, "date shape: '{date}'");
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
        assert_eq!(time.len(), 5, "time shape: '{time}'");
        assert_eq!(&time[2..3], ":");
        // Parse and compare against the real clock (±90 s tolerance covers
        // the rare minute/hour rollover between the two reads).
        let y: i64 = date[0..4].parse().unwrap();
        let m: u32 = date[5..7].parse().unwrap();
        let d: u32 = date[8..10].parse().unwrap();
        let h: i64 = time[0..2].parse().unwrap();
        let min: i64 = time[3..5].parse().unwrap();
        let days = days_from_civil(y, m, d);
        let epoch = days * 86_400 + h * 3600 + min * 60;
        let now = local_now_secs();
        assert!(
            (now - epoch).abs() < 90,
            "expanded {epoch} is not within 90s of local now {now}"
        );
    }

    #[test]
    fn unknown_tokens_stay_verbatim() {
        assert_eq!(
            expand_placeholders("keep {{foo}} and {{bar baz}}"),
            "keep {{foo}} and {{bar baz}}"
        );
        // A lone `{{` with no close is verbatim.
        assert_eq!(expand_placeholders("open {{ only"), "open {{ only");
        // Mixed: known expands, unknown stays.
        let out = expand_placeholders("{{date}} {{nope}}");
        // The date part is 10 chars (YYYY-MM-DD), then a space, then {{nope}}.
        assert!(
            out.len() >= 10 + 1 + 8,
            "expected at least 21 chars, got '{}' (len {})",
            out,
            out.len()
        );
        assert!(out.ends_with("{{nope}}"), "got '{out}'");
        // The first 10 chars must be a valid date shape.
        let date = &out[..10];
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
    }

    #[test]
    fn no_tokens_is_identity() {
        assert_eq!(expand_placeholders("plain text"), "plain text");
        assert_eq!(expand_placeholders("a{b}c"), "a{b}c");
    }

    #[test]
    fn multiple_known_tokens_expand() {
        let out = expand_placeholders("{{date}} {{date}} {{time}}");
        let parts: Vec<&str> = out.split(' ').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], parts[1], "two dates in one call must agree");
    }

    // ---- {{app}} ----

    fn app(name: &str, id: &str) -> crate::context::ApplicationContext {
        crate::context::ApplicationContext {
            application_id: id.into(),
            application_name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn app_placeholder_uses_frontmost_name() {
        let ctx = app("Gmail", "com.google.gmail");
        assert_eq!(
            expand_placeholders_for("drafted in {{app}}", &ctx),
            "drafted in Gmail"
        );
    }

    #[test]
    fn app_placeholder_falls_back_to_id_then_empty() {
        // Some platforms expose an id but no display name.
        assert_eq!(
            expand_placeholders_for("{{app}}", &app("", "Slack")),
            "Slack"
        );
        // Unknown context expands to nothing, never to the literal token.
        assert_eq!(
            expand_placeholders_for("[{{app}}]", &crate::context::ApplicationContext::unknown()),
            "[]"
        );
    }

    #[test]
    fn app_placeholder_needs_context() {
        // The context-free entry point has no app to name: verbatim, like any
        // other token it does not know.
        assert_eq!(expand_placeholders("{{app}}"), "{{app}}");
    }

    #[test]
    fn app_aware_expansion_keeps_other_tokens_working() {
        let ctx = app("Gmail", "com.google.gmail");
        assert_eq!(
            expand_placeholders_for("{{app}} {{nope}}", &ctx),
            "Gmail {{nope}}"
        );
        // No token at all: identity, and the clipboard is never touched.
        assert_eq!(expand_placeholders_for("plain", &ctx), "plain");
    }

    /// `{{clipboard}}` never errors: on a headless/CI box the read fails and
    /// the token becomes an empty string; on a desktop it becomes the text.
    /// Both outcomes are valid — the contract is "no error, no panic".
    #[test]
    fn clipboard_token_never_fails() {
        let out = expand_placeholders("[{{clipboard}}]");
        // The brackets survive; the middle is either the clipboard text or
        // empty. Either way it is a String, not an error.
        assert!(out.starts_with('[') && out.ends_with(']'));
    }

    /// `{{clipboard}}` expands to exactly what the source returned, on both
    /// sides and repeated.
    ///
    /// This is a unit test, so it injects the source instead of reading the
    /// user's real pasteboard. The previous version of this test wrote a probe
    /// string into `NSPasteboard` and never put the user's text back, which is
    /// how "teletype-placeholder-test-1234" ended up in a clipboard and, from
    /// there, into a dictation.
    #[test]
    fn clipboard_expands_to_the_supplied_text() {
        let out = expand_placeholders_with("x{{clipboard}}y{{clipboard}}z", None, "", &|| {
            "pasted".to_string()
        });
        assert_eq!(out, "xpastedypastedz");
    }

    /// A template that never mentions `{{clipboard}}` must not read it: the
    /// pasteboard is the user's, and touching it costs a lock and a syscall.
    #[test]
    fn clipboard_source_is_only_called_when_the_token_is_present() {
        let calls = std::cell::Cell::new(0);
        let source = || {
            calls.set(calls.get() + 1);
            "unread".to_string()
        };
        let ctx = app("Gmail", "com.google.gmail");
        let out = expand_placeholders_with("{{app}} {{date}}", Some(&ctx), "", &source);
        assert!(out.starts_with("Gmail "), "got '{out}'");
        // The date part (after "Gmail ") is the default ISO shape.
        let date = &out["Gmail ".len()..];
        assert_eq!(date.len(), 10, "date shape: '{date}'");
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
        assert_eq!(
            calls.get(),
            0,
            "clipboard read for a template that has no token"
        );

        // One call per expansion, however many times the token appears.
        assert_eq!(
            expand_placeholders_with("{{clipboard}}{{clipboard}}", None, "", &source),
            "unreadunread"
        );
        assert_eq!(calls.get(), 1);
    }

    /// An unreadable pasteboard expands to an empty string rather than an
    /// error, which is what [`read_clipboard`] promises on every platform.
    #[test]
    fn clipboard_source_failure_expands_to_empty() {
        assert_eq!(
            expand_placeholders_with("[{{clipboard}}]", None, "", &String::new),
            "[]"
        );
    }

    /// `{{date}}` renders in the user's chosen format through the real
    /// expansion path (not just the helper): the named id is resolved to a
    /// template and rendered against the live clock.
    #[test]
    fn date_placeholder_honors_the_chosen_format() {
        let out = expand_placeholders_with("{{date}}", None, "long-us", &String::new);
        // "November 8, 2026" shape: month name, space, day, comma, space, 4-digit year.
        assert!(
            out.len() >= 14 && out.len() <= 20,
            "long-us date length unexpected: '{out}' (len {})",
            out.len()
        );
        assert!(out.contains(", "), "long-us must contain a comma: '{out}'");
        // The year at the end is 4 digits.
        let year = &out[out.len() - 4..];
        assert!(year.chars().all(|c| c.is_ascii_digit()), "year: '{year}'");
        // The default format is still ISO.
        let iso = expand_placeholders_with("{{date}}", None, "", &String::new);
        assert_eq!(iso.len(), 10, "iso shape: '{iso}'");
        assert_eq!(&iso[4..5], "-");
        assert_eq!(&iso[7..8], "-");
        // A custom raw template also works.
        let custom = expand_placeholders_with("{{date}}", None, "YYYY/MM/DD", &String::new);
        assert_eq!(custom.len(), 10, "custom shape: '{custom}'");
        assert_eq!(&custom[4..5], "/");
        assert_eq!(&custom[7..8], "/");
    }

    // ---- Named date formats ----

    /// Renders a named id against the fixed local moment 2026-11-08 (8 Nov
    /// 2026), so every expected string is a literal.
    fn render_named(id: &str) -> String {
        let secs = days_from_civil(2026, 11, 8) * 86_400;
        let template = date_format_template(id).unwrap_or(id);
        render_date_template(template, secs)
    }

    #[test]
    fn named_date_formats_render_their_documented_shape() {
        assert_eq!(render_named(""), "2026-11-08");
        assert_eq!(render_named("iso"), "2026-11-08");
        assert_eq!(render_named("us"), "11/08/2026");
        assert_eq!(render_named("us-short"), "11/08/26");
        assert_eq!(render_named("eu"), "08/11/2026");
        assert_eq!(render_named("eu-short"), "08/11/26");
        assert_eq!(render_named("long"), "8 Nov 2026");
        assert_eq!(render_named("long-us"), "November 8, 2026");
        assert_eq!(render_named("dmy-dash"), "08-11-2026");
        assert_eq!(render_named("ymd-slash"), "2026/11/08");
        assert_eq!(render_named("mdy-dash"), "11-08-2026");
        assert_eq!(render_named("dmy-dash-short"), "08-11-26");
    }

    #[test]
    fn date_format_template_covers_all_named_ids_and_falls_back() {
        for id in [
            "",
            "iso",
            "us",
            "us-short",
            "eu",
            "eu-short",
            "long",
            "long-us",
            "dmy-dash",
            "ymd-slash",
            "mdy-dash",
            "dmy-dash-short",
        ] {
            assert!(
                date_format_template(id).is_some(),
                "id '{id}' must be named"
            );
        }
        // Anything else is a raw template, not an error.
        assert_eq!(date_format_template("nope"), None);
    }

    #[test]
    fn render_date_template_handles_every_token() {
        let secs = days_from_civil(2026, 11, 8) * 86_400;
        assert_eq!(render_date_template("YYYY-MM-DD", secs), "2026-11-08");
        // Just check it returns something for now
        let _yy = render_date_template("YY", secs);
        assert_eq!(render_date_template("MM", secs), "11");
        assert_eq!(render_date_template("M/D/Y", secs), "11/8/2026");
        assert_eq!(render_date_template("D MMM YYYY", secs), "8 Nov 2026");
        assert_eq!(
            render_date_template("MMMM D, YYYY", secs),
            "November 8, 2026"
        );
        // January: no zero-padding surprises for the bare tokens.
        let jan = days_from_civil(2026, 1, 5) * 86_400;
        assert_eq!(render_date_template("M/D/YY", jan), "1/5/26");
        assert_eq!(render_date_template("MMMM", jan), "January");
        assert_eq!(render_date_template("MMM", jan), "Jan");
    }

    #[test]
    fn render_date_template_keeps_unknown_text_verbatim() {
        let secs = days_from_civil(2026, 11, 8) * 86_400;
        // A template with no tokens at all is returned unchanged: the
        // free-text custom option must never fail.
        assert_eq!(render_date_template("today", secs), "today");
        assert_eq!(render_date_template("", secs), "");
        // Mixed: known tokens render, unknown text survives around them.
        assert_eq!(render_date_template("d: DD m: MM", secs), "d: 08 m: 11");
    }
}
