//! Closed-set placeholder expansion for AutoText replacement values.
//!
//! A replacement may contain `{{date}}`, `{{time}}` or `{{clipboard}}`;
//! each occurrence is expanded **at expansion time** (every use gets a fresh
//! value), never at save time. Unknown `{{...}}` tokens are left verbatim.
//! Expansion never fails: an unreadable clipboard becomes an empty string.
//!
//! The date/time math is dependency-free (same civil-date algorithm as
//! [`crate::history`]); the local timezone offset is probed at call time so
//! DST transitions are honored.

use std::time::SystemTime;

/// Expands the closed set of placeholders in `text`.
///
/// - `{{date}}` → local date, `YYYY-MM-DD`
/// - `{{time}}` → local time, `HH:MM` (24h)
/// - `{{clipboard}}` → current clipboard text (empty string on error)
///
/// Any other `{{...}}` is preserved exactly as written.
pub fn expand_placeholders(text: &str) -> String {
    if !text.contains("{{") {
        return text.to_string();
    }
    let (date, time) = now_local();
    let clipboard = read_clipboard();
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
                    "clipboard" => out.push_str(&clipboard),
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

/// The current local date and time as (`YYYY-MM-DD`, `HH:MM`).
fn now_local() -> (String, String) {
    let local = local_now_secs();
    let days = local.div_euclid(86_400);
    let rem = local.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let date = format!("{y:04}-{m:02}-{d:02}");
    let time = format!("{:02}:{:02}", rem / 3600, (rem % 3600) / 60);
    (date, time)
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
    match arboard::Clipboard::new() {
        Ok(mut cb) => match cb.get_text() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[autotext] {{clipboard}} unreadable ({e}); expanding to empty string");
                String::new()
            }
        },
        Err(e) => {
            eprintln!("[autotext] clipboard unavailable ({e}); expanding to empty string");
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

    #[test]
    fn clipboard_expands_to_current_text_when_available() {
        // Only meaningful when a clipboard is available; skip silently when
        // it isn't (CI / headless). On CI we skip *before* the first ObjC
        // call, because even a failing `Clipboard::new()` leaves a pending
        // foreign exception that aborts the process later.
        if is_headless() {
            return;
        }
        let mut cb = match arboard::Clipboard::new() {
            Ok(cb) => cb,
            Err(_) => return,
        };
        let probe = "teletype-placeholder-test-1234";
        if cb.set_text(probe.to_string()).is_err() {
            return;
        }
        let out = expand_placeholders("x{{clipboard}}y");
        assert_eq!(out, format!("x{probe}y"));
    }
}
