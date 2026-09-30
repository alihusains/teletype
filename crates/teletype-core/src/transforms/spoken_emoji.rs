//! Spoken-emoji formatter (BUG-002): turn spoken emoji phrases into glyphs.
//!
//! Ported from EW's `EmojiFormatterStep` / `EmojiFormatter` (Tier A/B only —
//! exact phrase + synonym match with a required trigger word; the phonetic
//! Tier C pass is intentionally not ported). A phrase only converts when the
//! user said "emoji" or "emoticon" right after it: "my heart" stays "my
//! heart", "send a heart emoji" becomes "send a ❤️".
//!
//! House pattern (see [`crate::emoji`]): a pure, stateless
//! `format_spoken_emoji(input, enabled) -> String` with table-driven tests.

/// The phrase → glyph table. Phrases never contain a trigger word; synonyms
/// are extra surfaces for the same phrase. Ported from EW's
/// `emoji-dictionary.json` (focused ~50-entry subset).
///
/// Matching is longest-surface-first so a shorter surface (e.g. "heart")
/// cannot shadow a longer one ("sparkling heart").
const TABLE: &[(&str, &str, &[&str])] = &[
    ("thumbs up", "👍", &["thumb up", "thumbs up sign"]),
    ("thumbs down", "👎", &["thumb down"]),
    ("red heart", "❤️", &[]),
    ("sparkling heart", "💖", &[]),
    ("heart eyes", "😍", &["heart eyes face"]),
    ("fire", "🔥", &[]),
    ("rocket", "🚀", &[]),
    ("smiling face", "🙂", &["smiley face", "smiley"]),
    ("smiling face with open mouth", "😀", &["grinning face"]),
    ("laughing face", "😂", &["face with tears of joy"]),
    ("wink", "😉", &["winking face"]),
    ("sad face", "😢", &["sad", "crying face"]),
    ("loud crying", "😭", &["sobbing face"]),
    ("angry face", "😠", &["angry"]),
    ("angry face with horns", "👿", &["imp"]),
    ("skull", "💀", &["skull and crossbones"]),
    ("ghost", "👻", &[]),
    ("alien", "👽", &["alien monster"]),
    ("thinking face", "🤔", &["thinking"]),
    ("shrug", "🤷", &["shrugging person"]),
    ("pleading face", "🥺", &[]),
    ("sleeping face", "😴", &["sleeping"]),
    ("yawning face", "🥱", &[]),
    ("exploding head", "🤯", &[]),
    ("face with rolling eyes", "🙄", &["rolling eyes"]),
    ("facepalm", "🤦", &[]),
    ("okay hand", "👌", &["ok hand", "ok gesture"]),
    ("raised hand", "✋", &["raised hand with fingers splayed"]),
    ("waving hand", "👋", &["wave", "waving hand sign"]),
    ("clapping hands", "👏", &["clap", "clapping hands sign"]),
    ("folded hands", "🙏", &["pray", "folded hands sign"]),
    ("muscle", "💪", &["flexed biceps"]),
    ("eyes", "👀", &[]),
    ("party popper", "🎉", &["tada"]),
    ("balloon", "🎈", &[]),
    ("trophy", "🏆", &[]),
    ("checkmark", "✅", &["check mark", "white check mark"]),
    ("cross mark", "❌", &["cross", "x mark"]),
    ("warning", "⚠️", &["warning sign"]),
    ("stop sign", "🛑", &[]),
    ("hundred", "💯", &["hundred points"]),
    ("star", "⭐", &["star shape"]),
    ("sparkles", "✨", &[]),
    ("money bag", "💰", &[]),
    ("calendar", "📅", &[]),
    ("alarm clock", "⏰", &[]),
    ("lightbulb", "💡", &["bulb"]),
    ("bomb", "💣", &[]),
    ("snowflake", "❄️", &[]),
    ("umbrella", "☂️", &[]),
    ("sun", "☀️", &[]),
    ("rainbow", "🌈", &[]),
    ("moon", "🌙", &[]),
    ("coffee", "☕", &["hot beverage"]),
    ("beer", "🍺", &[]),
    ("cake", "🎂", &["birthday cake"]),
];

/// A word boundary for phrase matching: start/end of text, or a run of
/// non-alphanumeric characters (space, punctuation).
fn is_boundary(c: Option<char>) -> bool {
    !c.is_some_and(|c| c.is_alphanumeric())
}

/// The full match span for one entry at `pos`, or `None`.
///
/// Shape: `<phrase> <sep>* (emoji|emoticon) <boundary>` — the trigger word is
/// required, so bare nouns never convert. `pos` must sit at a word boundary
/// (the caller advances one char at a time until a span matches).
fn find_match(chars: &[char], pos: usize) -> Option<(usize, &str)> {
    if !is_boundary(if pos == 0 { None } else { Some(chars[pos - 1]) }) {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;

    // All surfaces for this position, longest first, so a longer surface can
    // never be shadowed by a shorter prefix of it.
    let mut surfaces: Vec<(&str, &str)> = Vec::new();
    for (phrase, glyph, synonyms) in TABLE {
        surfaces.push((phrase, glyph));
        for s in *synonyms {
            surfaces.push((s, glyph));
        }
    }
    surfaces.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));

    for (surface, glyph) in surfaces {
        let Some(end) = match_surface(chars, pos, surface) else {
            continue;
        };
        // The trigger word "emoji"/"emoticon" must follow, separated by
        // whitespace/punctuation only.
        let mut i = end;
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',' || chars[i] == '.') {
            i += 1;
        }
        if let Some(trigger_end) = match_trigger(chars, i) {
            // First (longest) surface to match wins.
            best = Some((trigger_end, glyph));
            break;
        }
    }
    best
}

/// Match the trigger word ("emoji" or "emoticon") at `start`, requiring a
/// word boundary after it. Returns the end index (exclusive).
fn match_trigger(chars: &[char], start: usize) -> Option<usize> {
    for trigger in ["emoji", "emoticon"] {
        let t: Vec<char> = trigger.chars().collect();
        if start + t.len() > chars.len() {
            continue;
        }
        let mut ok = true;
        for (k, &want) in t.iter().enumerate() {
            if chars[start + k].to_ascii_lowercase() != want {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        if chars
            .get(start + t.len())
            .is_some_and(|c| c.is_alphanumeric())
        {
            continue; // "emojiface" is not a trigger
        }
        return Some(start + t.len());
    }
    None
}

/// Match `surface` (lowercase words) starting at `pos` in `chars`.
/// Words may be separated by whitespace/punctuation; each word must end at a
/// hard boundary. Returns the end index (exclusive) or `None`.
fn match_surface(chars: &[char], mut pos: usize, surface: &str) -> Option<usize> {
    for word in surface.split_whitespace() {
        let w: Vec<char> = word.chars().collect();
        if pos + w.len() > chars.len() {
            return None;
        }
        for (k, &want) in w.iter().enumerate() {
            let tc = chars[pos + k];
            if tc != want {
                return None;
            }
        }
        pos += w.len();
        if pos < chars.len() && chars[pos].is_alphanumeric() {
            return None; // ran into a longer word
        }
        // Consume the gap between words (whitespace or a lone comma).
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ',') {
            pos += 1;
        }
    }
    Some(pos)
}

/// Converts known spoken-emoji phrases to glyphs.
///
/// When `enabled` is false, returns `text` unchanged. When true, replaces
/// `<phrase> emoji|emoticon` spans (word-boundary, case-insensitive) with the
/// glyph, keeping one space between the glyph and adjacent words.
pub fn format_spoken_emoji(text: &str, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0;
    while pos < chars.len() {
        if let Some((end, glyph)) = find_match(&chars, pos) {
            // Spacing: no space after an opening bracket/quote, a space
            // before an adjacent word, no space before punctuation.
            let prev_is_space = chars[..pos].iter().next_back().is_some_and(|c| {
                c.is_whitespace() || *c == '(' || *c == '[' || *c == '{' || *c == '"'
            });
            let next = chars.get(end).copied();
            let next_is_space = next.is_some_and(|c| {
                c.is_whitespace()
                    || c == ')'
                    || c == ']'
                    || c == '}'
                    || c == '!'
                    || c == '.'
                    || c == ','
                    || c == '?'
                    || c == ';'
            });
            if !out.is_empty() && !prev_is_space {
                out.push(' ');
            }
            out.push_str(glyph);
            if next.is_some() && !next_is_space {
                out.push(' ');
            }
            pos = end;
        } else {
            out.push(chars[pos]);
            pos += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_identity() {
        assert_eq!(
            format_spoken_emoji("thanks for the thumbs up emoji", false),
            "thanks for the thumbs up emoji"
        );
    }

    #[test]
    fn converts_thumbs_up() {
        let out = format_spoken_emoji("thanks for the thumbs up emoji", true);
        assert!(out.contains('👍'), "got: {out}");
        assert_eq!(out, "thanks for the 👍");
    }

    #[test]
    fn bare_word_never_converts() {
        // No "emoji"/"emoticon" trigger: the word stays.
        assert_eq!(format_spoken_emoji("my heart", true), "my heart");
        assert_eq!(
            format_spoken_emoji("the star of the show", true),
            "the star of the show"
        );
        assert_eq!(
            format_spoken_emoji("fire drill at noon", true),
            "fire drill at noon"
        );
    }

    #[test]
    fn trigger_word_is_case_insensitive() {
        // The trigger word itself is case-insensitive…
        assert_eq!(format_spoken_emoji("sad face EMOJI", true), "😢");
        // …but the phrase surface is not: "Heart Emoji" is the English
        // heart-eyes face, not a red heart, so it must stay as words.
        assert_eq!(
            format_spoken_emoji("a Heart Emoji please", true),
            "a Heart Emoji please"
        );
        assert_eq!(format_spoken_emoji("sad face emoticon", true), "😢");
    }

    #[test]
    fn longest_surface_wins() {
        // "sparkling heart emoji" must not be eaten by a shorter surface.
        assert_eq!(format_spoken_emoji("a sparkling heart emoji", true), "a 💖");
    }

    #[test]
    fn multiple_in_one_text() {
        let out = format_spoken_emoji("great work, thumbs up emoji and clap emoji", true);
        assert_eq!(out, "great work, 👍 and 👏");
    }

    #[test]
    fn spacing_around_glyph() {
        assert_eq!(format_spoken_emoji("thumbs up emoji!", true), "👍!");
        assert_eq!(format_spoken_emoji("(waving hand emoji)", true), "(👋)");
        assert_eq!(
            format_spoken_emoji("waving hand emoji hello", true),
            "👋 hello"
        );
    }

    #[test]
    fn idempotent() {
        let once = format_spoken_emoji("a thumbs up emoji and a fire emoji", true);
        assert_eq!(format_spoken_emoji(&once, true), once);
    }
}
