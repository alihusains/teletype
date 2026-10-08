//! InputNormalizer: one place that turns raw key events into meaning.
//!
//! Pipeline position: `CGEventTap -> InputNormalizer -> ShortcutManager OR
//! AutoTextManager`. Shortcuts match on (keycode, modifier flags); AutoText
//! matches on resulting Unicode text, never on physical keycodes alone.

/// CGEventFlags bits for the four modifiers (same values as NSEvent).
pub const FLAG_SHIFT: u64 = 1 << 17;
pub const FLAG_CTRL: u64 = 1 << 18;
pub const FLAG_ALT: u64 = 1 << 19;
pub const FLAG_CMD: u64 = 1 << 20;

/// Mask of the modifier bits the engine compares.
pub const MOD_MASK: u64 = FLAG_SHIFT | FLAG_CTRL | FLAG_ALT | FLAG_CMD;

/// Map a Tauri main-key token to a macOS virtual keycode.
/// Covers letters, digits, common symbols, and the non-printing keys the
/// recorder emits. Returns `None` for keys the tap cannot map (caller falls
/// back to Carbon for those).
pub fn main_to_keycode(main: &str) -> Option<i64> {
    match main {
        "Space" => Some(49),
        "Enter" => Some(36),
        "Tab" => Some(48),
        "Escape" => Some(53),
        "Delete" => Some(51),
        "Left" => Some(123),
        "Right" => Some(124),
        "Down" => Some(125),
        "Up" => Some(126),
        "Home" => Some(115),
        "End" => Some(119),
        "PageUp" => Some(116),
        "PageDown" => Some(121),
        "F1" => Some(122),
        "F2" => Some(120),
        "F3" => Some(99),
        "F4" => Some(118),
        "F5" => Some(96),
        "F6" => Some(97),
        "F7" => Some(98),
        "F8" => Some(100),
        "F9" => Some(101),
        "F10" => Some(109),
        "F11" => Some(103),
        "F12" => Some(111),
        "-" | "Minus" => Some(27),
        "=" | "Equal" => Some(24),
        "[" => Some(33),
        "]" => Some(30),
        ";" => Some(41),
        "'" | "Quote" => Some(39),
        "," | "Comma" => Some(43),
        "." | "Period" => Some(47),
        "/" | "Slash" => Some(44),
        "`" | "Grave" => Some(50),
        "\\" | "Backslash" => Some(42),
        _ => {
            let up = main.to_ascii_uppercase();
            if up.len() != 1 {
                return None;
            }
            let c = up.chars().next().unwrap();
            match c {
                'A' => Some(0),
                'S' => Some(1),
                'D' => Some(2),
                'F' => Some(3),
                'H' => Some(4),
                'G' => Some(5),
                'Z' => Some(6),
                'X' => Some(7),
                'C' => Some(8),
                'V' => Some(9),
                'B' => Some(11),
                'Q' => Some(12),
                'W' => Some(13),
                'E' => Some(14),
                'R' => Some(15),
                'Y' => Some(16),
                'T' => Some(17),
                '1' => Some(18),
                '2' => Some(19),
                '3' => Some(20),
                '4' => Some(21),
                '6' => Some(22),
                '5' => Some(23),
                '9' => Some(25),
                '7' => Some(26),
                '8' => Some(28),
                '0' => Some(29),
                'O' => Some(31),
                'L' => Some(37),
                'J' => Some(38),
                'K' => Some(40),
                'N' => Some(45),
                'M' => Some(46),
                'P' => Some(35),
                'I' => Some(34),
                'U' => Some(32),
                _ => None,
            }
        }
    }
}

/// Parse `Alt+1` / `Cmd+Shift+Space` into `(flags, keycode)`.
/// Returns `None` for bare-modifier strings, `Fn`, empty, or unmapped keys.
pub fn parse_shortcut(shortcut: &str) -> Option<(u64, i64)> {
    let trimmed = shortcut.trim();
    if trimmed.is_empty() || trimmed == "Fn" {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('+').collect();
    if parts.is_empty() {
        return None;
    }
    let bare = ["Cmd", "Ctrl", "Alt", "Shift", "Fn"];
    if parts.iter().all(|p| bare.contains(p)) {
        return None;
    }
    let main = *parts.last().unwrap();
    if bare.contains(&main) {
        return None;
    }
    let mut flags: u64 = 0;
    for m in &parts[..parts.len() - 1] {
        match *m {
            "Cmd" => flags |= FLAG_CMD,
            "Ctrl" => flags |= FLAG_CTRL,
            "Alt" => flags |= FLAG_ALT,
            "Shift" => flags |= FLAG_SHIFT,
            _ => return None,
        }
    }
    let kc = main_to_keycode(main)?;
    Some((flags, kc))
}

/// Kinds forwarded by the native tap for observed (non-shortcut) KeyDowns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedKind {
    /// Printable resulting text (layout-aware Unicode from the event).
    Text,
    /// Backward delete; edits the recent-text buffer.
    Backspace,
    /// Word delimiter (space/tab/enter); completes a trigger candidate.
    Delimiter,
    /// Navigation / escape / function key: breaks the current candidate.
    Break,
}

impl ObservedKind {
    pub fn from_raw(kind: u32) -> Self {
        match kind {
            1 => Self::Backspace,
            2 => Self::Delimiter,
            3 => Self::Break,
            _ => Self::Text,
        }
    }
}

/// Decode UTF-16 units from the tap into a Rust String (lossy, never panics).
pub fn decode_utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_alt_digit() {
        let (flags, kc) = parse_shortcut("Alt+1").expect("Alt+1 must parse");
        assert_eq!(flags, FLAG_ALT);
        assert_eq!(kc, 18);
    }

    #[test]
    fn parses_alt_letter() {
        let (flags, kc) = parse_shortcut("Alt+T").expect("Alt+T must parse");
        assert_eq!(flags, FLAG_ALT);
        assert_eq!(kc, 17);
    }

    #[test]
    fn parses_cmd_shift_space() {
        let (flags, kc) = parse_shortcut("Cmd+Shift+Space").expect("must parse");
        assert_eq!(flags, FLAG_CMD | FLAG_SHIFT);
        assert_eq!(kc, 49);
    }

    #[test]
    fn rejects_bare_modifiers_and_fn() {
        assert!(parse_shortcut("Alt").is_none());
        assert!(parse_shortcut("Cmd+Shift").is_none());
        assert!(parse_shortcut("Fn").is_none());
        assert!(parse_shortcut("").is_none());
    }

    #[test]
    fn masks_only_modifier_bits() {
        let raw = FLAG_ALT | FLAG_CMD | (1 << 23) | (1 << 16);
        assert_eq!(raw & MOD_MASK, FLAG_ALT | FLAG_CMD);
    }

    #[test]
    fn decodes_emoji_units() {
        let units: Vec<u16> = "😀".encode_utf16().collect();
        assert_eq!(decode_utf16(&units), "😀");
    }
}
