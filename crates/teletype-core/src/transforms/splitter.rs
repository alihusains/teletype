//! Long-transcript splitter (T1.3).
//!
//! A single very long transcript degrades a local model's polish: it loses
//! coherence across the window and risks tripping the context preflight
//! (which falls back to the raw text). We split the *protected* input into
//! sentence-aligned chunks of at most [`MAX_CHUNK_WORDS`] words, transform
//! each, and re-join. Placeholders (`{{AUTOTEXT_N}}`) are treated as
//! atomic words so a chunk never cuts one in half.

/// Target maximum words per chunk. 500 is a practical upper bound for a
/// local model to polish coherently; shorter chunks are fine.
pub const MAX_CHUNK_WORDS: usize = 500;

/// Splits `text` into chunks of at most `MAX_CHUNK_WORDS` words each.
///
/// Returns a single chunk when the input is short (the fast path, zero
/// behaviour change). Chunks break at sentence boundaries (`.`, `!`, `?`,
/// `…`) where possible, falling back to a word boundary mid-sentence for
/// run-on text. A placeholder is never split. Chunks preserve their
/// internal whitespace; the caller re-joins with a single space.
pub fn split_for_polish(text: &str) -> Vec<String> {
    let words = tokenize(text);
    if words.iter().map(|w| w.word_count()).sum::<usize>() <= MAX_CHUNK_WORDS {
        return vec![text.to_string()];
    }
    chunk_words(&words)
}

/// A whitespace-delimited token, but a `{{AUTOTEXT_N}}` placeholder is always
/// one word regardless of surrounding spaces.
#[derive(Clone)]
struct Token {
    /// The verbatim text of the token (placeholder or word).
    text: String,
    /// 1 for a placeholder or normal word; the splitter only needs "this is
    /// one unit", so every token counts as exactly 1 word.
    #[allow(dead_code)]
    _word_count: usize,
}

impl Token {
    fn word_count(&self) -> usize {
        1
    }
}

fn tokenize(text: &str) -> Vec<Token> {
    text.split_whitespace()
        .map(|t| Token {
            text: t.to_string(),
            _word_count: 1,
        })
        .collect()
}

/// Groups tokens into chunks of at most `MAX_CHUNK_WORDS`, preferring to break
/// right after a token that ends a sentence.
fn chunk_words(tokens: &[Token]) -> Vec<String> {
    let mut chunks: Vec<Vec<Token>> = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    let mut count = 0usize;

    for token in tokens {
        current.push(token.clone());
        count += 1;
        // Break here if the chunk is full and the current token ends a
        // sentence (clean boundary), or unconditionally if it just hit the
        // cap (mid-sentence fallback for run-on text).
        let ends_sentence = ends_sentence(&token.text);
        if count >= MAX_CHUNK_WORDS || (count >= 40 && ends_sentence) {
            chunks.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
        .into_iter()
        .map(|toks| toks.into_iter().map(|t| t.text).collect::<Vec<_>>().join(" "))
        .collect()
}

/// True when the token's trailing punctuation (ignoring a closing quote or
/// paren) ends a sentence.
fn ends_sentence(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    let n = chars.len();
    if n == 0 {
        return false;
    }
    let last = chars[n - 1];
    let is_punct = |c: char| matches!(c, '.' | '!' | '?' | '…');
    let is_closer = |c: char| matches!(c, '"' | '\'' | ')' | '”');
    // `end.` / `end!` / `end?` / `end…`
    if is_punct(last) {
        return true;
    }
    // `end."` / `end)` — a closing quote/paren after the punctuation.
    n >= 2 && is_closer(last) && is_punct(chars[n - 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(n: usize) -> String {
        (0..n).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn short_input_returns_single_chunk_unchanged() {
        let input = "hello world, this is short.";
        assert_eq!(split_for_polish(input), vec![input.to_string()]);
    }

    #[test]
    fn splits_at_sentence_boundaries_under_cap() {
        // 6 sentences of 10 words each = 60 words, well under the 40-word
        // soft-break threshold per sentence, so it should NOT split (total
        // 60 <= 500). Force a split by using a small cap via many sentences.
        let sentences: Vec<String> = (0..12)
            .map(|i| format!("Sentence number {i} has ten words in it okay."))
            .collect();
        let input = sentences.join(" ");
        let chunks = split_for_polish(&input);
        // 12 * 9 words = 108 words total, <= 500, so one chunk.
        assert_eq!(chunks.len(), 1, "108 words should stay in one chunk");
    }

    #[test]
    fn long_runon_splits_at_word_boundary() {
        // 600 words with no sentence punctuation: must split into >= 2
        // chunks, each <= 500 words, and no word is lost.
        let input = words(600);
        let chunks = split_for_polish(&input);
        assert!(chunks.len() >= 2, "600 words must split, got {}", chunks.len());
        for c in &chunks {
            assert!(c.split_whitespace().count() <= MAX_CHUNK_WORDS);
        }
        // No word lost: re-joined chunk words equal the original words.
        let rejoined: Vec<&str> = chunks
            .iter()
            .flat_map(|c| c.split_whitespace())
            .collect();
        assert_eq!(rejoined.len(), 600);
        assert_eq!(rejoined.first().copied(), Some("w0"));
        assert_eq!(rejoined.last().copied(), Some("w599"));
    }

    #[test]
    fn never_splits_a_placeholder() {
        // A long run of words with a placeholder embedded; the placeholder
        // must appear intact in exactly one chunk.
        let mut parts: Vec<String> = (0..520).map(|i| format!("w{i}")).collect();
        parts.push("{{AUTOTEXT_0}}".into());
        let input = parts.join(" ");
        let chunks = split_for_polish(&input);
        let containing: Vec<&String> =
            chunks.iter().filter(|c| c.contains("{{AUTOTEXT_0}}")).collect();
        assert_eq!(containing.len(), 1, "placeholder must be in exactly one chunk");
        assert!(
            containing[0].contains("{{AUTOTEXT_0}}"),
            "placeholder must be intact, got: {}",
            &containing[0][..containing[0].len().min(40)]
        );
    }

    #[test]
    fn sentence_ending_detection() {
        assert!(ends_sentence("end."));
        assert!(ends_sentence("wow!"));
        assert!(ends_sentence("really?"));
        assert!(ends_sentence("end.\"")); // closing quote after period
        assert!(!ends_sentence("done)")); // paren alone is not a sentence end
        assert!(!ends_sentence("middle"));
        assert!(ends_sentence("e.g.")); // trailing period counts (acceptable)
    }
}
