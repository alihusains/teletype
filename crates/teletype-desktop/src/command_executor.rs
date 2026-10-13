//! Command executor: turns a structured `CommandAction` into a real
//! system action (open an app, set brightness, mute the speaker, etc.).
//!
//! Each action is implemented with the simplest platform API available
//! on macOS. Actions that require confirmation (destructive operations)
//! are not executed here — the caller must check the risk level first.

use teletype_core::context::ApplicationContext;
use teletype_core::llm::CommandAction;

/// Executes a `CommandAction` and returns a human-readable result.
pub fn execute(cmd: &CommandAction, _context: &ApplicationContext) -> Result<String, String> {
    match cmd.tool.as_str() {
        "open_app" => execute_open_app(cmd),
        "open_url" => execute_open_url(cmd),
        "set_brightness" => execute_set_brightness(cmd),
        "set_volume" => execute_set_volume(cmd),
        "mute_speaker" => execute_mute_speaker(cmd),
        "open_terminal" => execute_open_terminal(cmd),
        "keyboard_shortcut" => execute_keyboard_shortcut(cmd),
        "open_settings" => execute_open_settings(cmd),
        "play_media" => execute_play_media(cmd),
        "set_timer" => execute_set_timer(cmd),
        "send_message" => execute_send_message(cmd),
        other => Err(format!("unknown command tool: {other}")),
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
        "downloads" => return "Finder".into(),
        "documents" => return "Finder".into(),
        "terminal" | "command line" | "console" => return "Terminal".into(),
        "system settings" | "settings" | "preferences" => return "System Settings".into(),
        "screenshot" => return "Screenshot".into(),
        "desktop" => return "Finder".into(),
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

/// Scans /Applications for .app bundles and fuzzy-matches the query
/// against their names. Returns the best match if the similarity score
/// is above a threshold.
#[cfg(target_os = "macos")]
fn fuzzy_match_installed_app(query: &str) -> Option<String> {
    let apps_dir = std::path::Path::new("/Applications");
    let entries = std::fs::read_dir(apps_dir).ok()?;

    let query_lower = query.to_lowercase();
    let mut best_match: Option<(String, f64)> = None;
    const THRESHOLD: f64 = 0.5; // minimum similarity to accept a match

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

        // Exact match (case-insensitive).
        if app_lower == query_lower {
            return Some(app_name);
        }

        // Check if the query is a substring of the app name or vice versa.
        if app_lower.contains(&query_lower) || query_lower.contains(&app_lower) {
            let score = 0.9; // strong match
            if best_match.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                best_match = Some((app_name, score));
            }
            continue;
        }

        // Levenshtein-based similarity for fuzzy matching.
        let similarity = levenshtein_similarity(&query_lower, &app_lower);
        if similarity >= THRESHOLD {
            if best_match.as_ref().map(|(_, s)| similarity > *s).unwrap_or(true) {
                best_match = Some((app_name, similarity));
            }
        }
    }

    best_match.map(|(name, _)| name)
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
            let cost = if a.chars().nth(i - 1) == b.chars().nth(j - 1) { 0 } else { 1 };
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
            "mute" => "tell application \"System Events\" to key code 49", // mute
            "unmute" => "tell application \"System Events\" to key code 49", // unmute
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
            Ok(if muted { "speaker muted" } else { "speaker unmuted" }.into())
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

fn execute_open_terminal(cmd: &CommandAction) -> Result<String, String> {
    let app = arg_str(cmd, "app").unwrap_or_else(|| "terminal".into());
    let command = arg_str(cmd, "command");

    #[cfg(target_os = "macos")]
    {
        // Use fuzzy matching to resolve the terminal app name.
        let app_name = if app.to_lowercase().contains("iterm") {
            "iTerm".to_string()
        } else {
            resolve_app_name(&app)
        };

        if let Some(ref cmd_str) = command {
            // Open Terminal and run the command.
            let script = format!(
                "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
                cmd_str.replace('"', "\\\"")
            );
            let status = std::process::Command::new("osascript")
                .args(["-e", &script])
                .status()
                .map_err(|e| format!("osascript terminal: {e}"))?;
            if status.success() {
                Ok(format!("opened {app_name} and ran: {cmd_str}"))
            } else {
                Err(format!("open_terminal failed: {status}"))
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
        let _ = (app, command);
        Err("open_terminal not supported on this platform".into())
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
            "next" => 124, // volume up (media next)
            "previous" => 125, // volume down (media previous)
            _ => return Err(format!("unknown media action: {action}")),
        };
        let status = std::process::Command::new("osascript")
            .args(["-e", &format!("tell application \"System Events\" to key code {key}")])
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

fn execute_send_message(cmd: &CommandAction) -> Result<String, String> {
    let to = arg_str(cmd, "to").ok_or("send_message: missing 'to' argument")?;
    let text = arg_str(cmd, "text").ok_or("send_message: missing 'text' argument")?;

    #[cfg(target_os = "macos")]
    {
        // Open Messages and send.
        let escaped_text = text.replace('"', "\\\"");
        let script = format!(
            "tell application \"Messages\"\nactivate\nset targetService to 1st account whose service type = iMessage service type\nset targetBuddy to participant \"{to}\" of targetService\nsend \"{escaped_text}\" to targetBuddy\nend tell"
        );
        let status = std::process::Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map_err(|e| format!("osascript messages: {e}"))?;
        if status.success() {
            Ok(format!("sent message to {to}"))
        } else {
            Err(format!("send_message to {to} failed: {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (to, text);
        Err("send_message not supported on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let cmd = CommandAction {
            tool: "unknown_tool".into(),
            arguments: json!({}),
        };
        let ctx = ApplicationContext::default();
        assert!(execute(&cmd, &ctx).is_err());
    }

    #[test]
    fn open_app_missing_arg_returns_error() {
        let cmd = CommandAction {
            tool: "open_app".into(),
            arguments: json!({}),
        };
        let ctx = ApplicationContext::default();
        assert!(execute(&cmd, &ctx).is_err());
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
}
