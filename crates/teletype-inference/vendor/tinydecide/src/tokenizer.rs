//! Lowercase WordPiece, the same normalisation as `WordPiece.encode` in tinydecide.js.
//!
//! Per code point: drop NUL, U+FFFD and control/format characters (tab, newline and CR stay as
//! spaces); space separators become spaces; CJK ideographs become one-character words; everything
//! else is NFD-decomposed, nonspacing marks are dropped, and each remaining character is lowercased.
//! Words split on whitespace (as JavaScript's `\s` sees it) and on punctuation (the ASCII symbol
//! ranges plus general category P*), then each word is split greedily into vocabulary pieces.

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;
use unicode_properties::general_category::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

use crate::error::{Error, Result};

/// Token ids plus the byte range of `text` each token came from.
#[derive(Debug, Clone, Default)]
pub struct Encoded {
    pub ids: Vec<u32>,
    /// `(start, end)` UTF-8 byte offsets into the encoded text.
    pub offsets: Vec<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub struct WordPiece {
    vocab: HashMap<String, u32>,
    unk: u32,
    prefix: String,
    max_chars: usize,
}

fn is_cjk(cp: u32) -> bool {
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x20000..=0x2A6DF).contains(&cp)
        || (0x2A700..=0x2B73F).contains(&cp)
        || (0x2B740..=0x2B81F).contains(&cp)
        || (0x2B820..=0x2CEAF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0x2F800..=0x2FA1F).contains(&cp)
}

fn is_punct(ch: char) -> bool {
    let c = ch as u32;
    if (33..=47).contains(&c) || (58..=64).contains(&c) || (91..=96).contains(&c) || (123..=126).contains(&c) {
        return true;
    }
    ch.general_category_group() == GeneralCategoryGroup::Punctuation
}

/// JavaScript's `\s` (WhiteSpace + LineTerminator).
pub(crate) fn is_js_space(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
            | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

type Ch = (char, usize, usize);

impl WordPiece {
    /// `vocab[i]` is the piece for id `i` (`None` for unused ids). Later duplicates win, as in the JS.
    pub fn new(vocab: &[Option<String>], unk: &str, prefix: &str, max_chars: usize) -> Result<Self> {
        let mut map = HashMap::with_capacity(vocab.len());
        for (i, p) in vocab.iter().enumerate() {
            if let Some(p) = p {
                map.insert(p.clone(), i as u32);
            }
        }
        let unk = *map.get(unk).ok_or_else(|| Error::Model(format!("unknown-token piece {unk:?} is not in the vocabulary")))?;
        Ok(WordPiece { vocab: map, unk, prefix: prefix.to_string(), max_chars })
    }

    pub fn unk(&self) -> u32 {
        self.unk
    }

    pub fn encode(&self, text: &str) -> Encoded {
        let mut chars: Vec<Ch> = Vec::with_capacity(text.len() + 8);
        for (a, ch) in text.char_indices() {
            let b = a + ch.len_utf8();
            let cp = ch as u32;
            let cat = ch.general_category();
            if cp == 0
                || cp == 0xFFFD
                || (matches!(cat, GeneralCategory::Control | GeneralCategory::Format) && !matches!(ch, '\t' | '\n' | '\r'))
            {
                continue;
            }
            if matches!(ch, ' ' | '\t' | '\n' | '\r') || cat == GeneralCategory::SpaceSeparator {
                chars.push((' ', a, b));
                continue;
            }
            if is_cjk(cp) {
                chars.push((' ', a, a));
                chars.push((ch, a, b));
                chars.push((' ', b, b));
                continue;
            }
            for d in std::iter::once(ch).nfd() {
                if d.general_category() == GeneralCategory::NonspacingMark {
                    continue;
                }
                for l in d.to_lowercase() {
                    chars.push((l, a, b));
                }
            }
        }

        let mut words: Vec<Vec<Ch>> = Vec::new();
        let mut cur: Vec<Ch> = Vec::new();
        for c in chars {
            if c.0 == ' ' || is_js_space(c.0) {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                continue;
            }
            if is_punct(c.0) {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                words.push(vec![c]);
                continue;
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            words.push(cur);
        }

        let mut out = Encoded::default();
        let mut s = String::new();
        let mut pieces: Vec<(u32, usize, usize)> = Vec::new();
        for w in &words {
            let whole = (w[0].1, w[w.len() - 1].2);
            if w.len() > self.max_chars {
                out.ids.push(self.unk);
                out.offsets.push(whole);
                continue;
            }
            pieces.clear();
            let mut start = 0;
            let mut bad = false;
            while start < w.len() {
                let mut end = w.len();
                let mut got = None;
                while start < end {
                    s.clear();
                    if start > 0 {
                        s.push_str(&self.prefix);
                    }
                    s.extend(w[start..end].iter().map(|c| c.0));
                    if let Some(&id) = self.vocab.get(s.as_str()) {
                        got = Some(id);
                        break;
                    }
                    end -= 1;
                }
                match got {
                    Some(id) => {
                        pieces.push((id, w[start].1, w[end - 1].2));
                        start = end;
                    }
                    None => {
                        bad = true;
                        break;
                    }
                }
            }
            if bad {
                out.ids.push(self.unk);
                out.offsets.push(whole);
            } else {
                for &(id, a, b) in &pieces {
                    out.ids.push(id);
                    out.offsets.push((a, b));
                }
            }
        }
        out
    }
}
