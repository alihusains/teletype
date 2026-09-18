//! Abstract keyboard shortcuts.
//!
//! Platform modifier names differ (⌘ vs Ctrl vs Win), so shortcuts are stored
//! abstractly and rendered/registered per platform.

use serde::{Deserialize, Serialize};

/// Abstract modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Modifiers {
    /// Cmd / Win.
    pub primary: bool,
    /// Option / Alt.
    pub option: bool,
    pub control: bool,
    pub shift: bool,
}

impl Modifiers {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_empty(self) -> bool {
        !self.primary && !self.option && !self.control && !self.shift
    }
}

/// One abstract shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    pub modifiers: Modifiers,
    /// Key name, e.g. "Space", "1", "F9".
    pub key: String,
}

impl Hotkey {
    pub fn new(modifiers: Modifiers, key: impl Into<String>) -> Self {
        Self {
            modifiers,
            key: key.into(),
        }
    }
}

/// Parses a platform accelerator string ("Cmd+Shift+Space", "Ctrl+Shift+Space")
/// into an abstract [`Hotkey`].
pub fn parse(accelerator: &str) -> Result<Hotkey, String> {
    let mut modifiers = Modifiers::none();
    let mut key: Option<String> = None;
    for part in accelerator.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "win" | "windows" | "meta" => modifiers.primary = true,
            "option" | "alt" => modifiers.option = true,
            "control" | "ctrl" => modifiers.control = true,
            "shift" => modifiers.shift = true,
            "fn" => return Err("Fn alone isn't supported yet".into()),
            other => {
                if key.is_some() {
                    return Err(format!("More than one key in '{accelerator}'"));
                }
                // Capitalize the first letter for display consistency.
                let capitalized: String = other
                    .chars()
                    .enumerate()
                    .map(|(i, c)| {
                        if i == 0 {
                            c.to_uppercase().to_string()
                        } else {
                            c.to_lowercase().to_string()
                        }
                    })
                    .collect();
                key = Some(capitalized);
            }
        }
    }
    let Some(key) = key else {
        return Err(format!("'{accelerator}' has no key"));
    };
    Ok(Hotkey { modifiers, key })
}

/// Renders an abstract hotkey for the current platform.
#[cfg(target_os = "macos")]
pub fn render(hotkey: &Hotkey) -> String {
    let mut s = String::new();
    if hotkey.modifiers.primary {
        s.push('⌘');
    }
    if hotkey.modifiers.option {
        s.push('⌥');
    }
    if hotkey.modifiers.control {
        s.push('⌃');
    }
    if hotkey.modifiers.shift {
        s.push('⇧');
    }
    s.push_str(&render_key(&hotkey.key));
    s
}

#[cfg(not(target_os = "macos"))]
pub fn render(hotkey: &Hotkey) -> String {
    let mut s = String::new();
    if hotkey.modifiers.primary {
        s.push_str("Ctrl+");
    }
    if hotkey.modifiers.option {
        s.push_str("Alt+");
    }
    if hotkey.modifiers.control {
        s.push_str("Ctrl+");
    }
    if hotkey.modifiers.shift {
        s.push_str("Shift+");
    }
    s.push_str(&render_key(&hotkey.key));
    s
}

fn render_key(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "space" => "Space".into(),
        "escape" => "Esc".into(),
        "return" | "enter" => "Return".into(),
        other => other.to_string(),
    }
}

/// Detects conflicts between two hotkeys.
pub fn conflicts(a: &Hotkey, b: &Hotkey) -> bool {
    a.modifiers == b.modifiers && a.key.eq_ignore_ascii_case(&b.key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mac_accelerator() {
        let h = parse("Cmd+Shift+Space").unwrap();
        assert!(h.modifiers.primary && h.modifiers.shift);
        assert_eq!(h.key, "Space");
    }

    #[test]
    fn parses_windows_accelerator() {
        let h = parse("Ctrl+Shift+Space").unwrap();
        assert!(h.modifiers.control && h.modifiers.shift);
        assert_eq!(h.key, "Space");
    }

    #[test]
    fn rejects_missing_key() {
        assert!(parse("Cmd+Shift").is_err());
    }

    #[test]
    fn conflict_detection() {
        let a = parse("Cmd+Shift+Space").unwrap();
        let b = parse("Cmd+Shift+Space").unwrap();
        let c = parse("Cmd+Shift+1").unwrap();
        assert!(conflicts(&a, &b));
        assert!(!conflicts(&a, &c));
    }
}
