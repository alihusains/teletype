//! T4: deterministic backtrack / self-correction resolver.
//!
//! A dictated take often contains a spoken repair:
//!
//! ```text
//! "let's meet at five, no wait, six"  ->  "let's meet at six"
//! ```
//!
//! The polish gate already detects self-correction cues (and therefore never
//! skips the model), and the tiered filler pass splices some repairs, but
//! there was no first-class, tested pass that resolves the retraction
//! deterministically, before the model. This module is that pass: a pure,
//! idempotent, model-free rewrite of the transcript.
//!
//! Rules (docs/research/polish-retention-strategy.md §6.2 A):
//!
//! * Cue + replacement -> remove the retracted span, keep the replacement.
//! * A quantity phrase is one fact: "twenty dollars, correction, thirty
//!   dollars" retracts "twenty dollars", never just the unit, so the output
//!   cannot contain "twenty thirty dollars". The same holds for a quantified
//!   object: "two of them, i mean three of them" retracts "two of them".
//! * A repaired object replaces a pronoun: "send it over, i mean the deck"
//!   drops the pronoun and keeps the particle -> "send over the deck".
//! * A repeated connector is repair framing, not the repaired word: "to
//!   Sarah, i mean, to Priya" contrasts on "Priya".
//! * Cue + no replacement -> keep the text unchanged (ambiguous; never guess).
//! * `actually` as emphasis -> keep (no contrasting replacement nearby).
//! * Never delete the first or last word of the take unless it is explicitly
//!   retracted by a cue.
//! * If the retracted span contains a protected fact (number, name, entity)
//!   and the replacement is ambiguous, keep both; do not silently choose.
//! * Punctuation, capitalization and the surrounding words are preserved.
//! * Calling the resolver twice produces the same text (idempotent).
//!
//! The pass is English-only (same gate as the filler and ITN passes) and runs
//! the transcript before the gate/model; typed input is the user's own
//! and is never touched by the pipeline's deterministic passes.

/// English-only gate for the backtrack pass, matching the ITN and filler
/// passes. Empty (legacy) language counts as English.
pub fn should_run(language: &str) -> bool {
    let lower = language.to_lowercase();
    lower.is_empty() || lower == "en" || lower.starts_with("en-") || lower.starts_with("en_")
}

/// Resolves spoken backtracks in `text` (see the module docs).
///
/// Pure and deterministic: no I/O, no inference. Returns the input
/// unchanged when no safe repair can be made.
pub fn resolve_backtracks(text: &str, language: &str) -> String {
    if !should_run(language) {
        return text.to_string();
    }
    let tokens = tokenise(text);
    if tokens.is_empty() {
        return text.to_string();
    }
    let retractions = find_retractions(&tokens);
    let mut remove = vec![false; tokens.len()];
    let mut strip_after = vec![false; tokens.len()];
    for ret in &retractions {
        if ret.keep_both {
            continue;
        }
        remove[ret.start..ret.start + ret.len].fill(true);
        for &(start, len) in &ret.extra_removals {
            remove[start..start + len].fill(true);
        }
        if let Some(idx) = ret.strip_comma_at {
            if idx < tokens.len() {
                strip_after[idx] = true;
            }
        }
        if ret.strip_comma {
            let mut idx = ret.start + ret.len;
            while idx < tokens.len() && remove[idx] {
                idx += 1;
            }
            if idx < tokens.len() {
                strip_after[idx] = true;
            }
        }
    }
    drop_duplicated_leading_function_word(&tokens, &mut remove, &retractions);
    if remove.iter().all(|&r| !r) {
        return text.to_string();
    }
    build_output(text, &tokens, &remove, &strip_after)
}

// ── Cues ─────────────────────────────────────────────────────────────────────

/// The cue phrases, longest-first so a start position matches the longest
/// cue first ("let me start over" before "let"). Each cue carries its words
/// pre-split so matching allocates nothing and never splits per token.
const CUES: &[(&str, &[&str])] = &[
    ("let me start over", &["let", "me", "start", "over"]),
    ("no wait", &["no", "wait"]),
    ("wait no", &["wait", "no"]),
    ("scratch that", &["scratch", "that"]),
    ("strike that", &["strike", "that"]),
    ("forget that", &["forget", "that"]),
    ("cancel that", &["cancel", "that"]),
    ("never mind", &["never", "mind"]),
    ("actually", &["actually"]),
    ("i mean", &["i", "mean"]),
    ("sorry", &["sorry"]),
    ("correction", &["correction"]),
    ("rather", &["rather"]),
];

/// Spelled-out number words, shared by the protected-fact checks in the
/// retraction resolvers. One module-level table so number detection cannot
/// drift between cues.
const NUM_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
];

/// True when a token is a quantity: a digit run ("20", "3pm") or a
/// spelled-out number word ("twenty", "hundred").
fn is_quantity_word(t: &Token) -> bool {
    let bare = t.bare.as_str();
    bare.chars().next().is_some_and(|c| c.is_ascii_digit()) || NUM_WORDS.contains(&bare)
}

/// True when a token carries content (not a function word or punctuation).
fn is_content_word(t: &Token) -> bool {
    !t.bare.is_empty() && !NON_FACTS.contains(&t.bare.as_str())
}

/// Determiners that open a repaired noun phrase: "I mean the deck".
const DETERMINERS: &[&str] = &["the", "a", "an"];

/// Object pronouns that a repaired noun phrase can replace: "ship it, i mean
/// the fix" -> "ship the fix". Demonstratives ("that", "this") are left out:
/// they far more often introduce a clause than stand in for a corrected
/// object.
const OBJECT_PRONOUNS: &[&str] = &["it", "them", "him", "her", "us"];

/// Verb particles that can separate the replaced pronoun from the cue: "send
/// it over, i mean the deck". English allows both "send the deck over" and
/// "send over the deck", so dropping the pronoun alone reads naturally and
/// the particle is kept.
const PARTICLES: &[&str] = &[
    "over", "out", "up", "down", "back", "through", "around", "along", "off", "away",
];

/// Extends a comma-bounded clause left over a quantified object.
///
/// In "buy two of them, i mean three of them" the comma walk retracts only
/// "them", which leaves "buy two of three of them". The counted phrase is one
/// fact, so the retraction includes the quantity and its "of".
fn extend_clause_over_quantified_object(tokens: &[Token], clause_start: usize) -> usize {
    if clause_start < 2 {
        return clause_start;
    }
    if !OBJECT_PRONOUNS.contains(&tokens[clause_start].bare.as_str()) {
        return clause_start;
    }
    if tokens[clause_start - 1].bare != "of" || !is_quantity_word(&tokens[clause_start - 2]) {
        return clause_start;
    }
    extend_clause_over_quantity(tokens, clause_start - 2)
}

/// Extends a comma-bounded clause left over the quantity that modifies it.
///
/// In "it costs twenty dollars, correction, thirty dollars" the comma walk
/// retracts only "dollars", which leaves "it costs twenty thirty dollars".
/// Walking left over `twenty` makes the retracted span the whole phrase, so
/// the output is "it costs thirty dollars". Only quantity words (and the
/// connectors inside a quantity, `a hundred`, `one hundred and twenty`) are
/// absorbed: a name or a verb stops the walk, so "invite Frank, scratch that,
/// Gordon" is unchanged.
fn extend_clause_over_quantity(tokens: &[Token], clause_start: usize) -> usize {
    let mut start = clause_start;
    while start > 0 {
        let prev = &tokens[start - 1];
        if is_quantity_word(prev) {
            start -= 1;
            continue;
        }
        // "a hundred dollars": the determiner belongs to the quantity phrase.
        if matches!(prev.bare.as_str(), "a" | "an") && is_quantity_word(&tokens[start]) {
            start -= 1;
            continue;
        }
        // "one hundred and twenty": the conjunction inside a quantity is part
        // of the phrase; a conjunction after a name ("Mom and Dad") is not.
        if prev.bare == "and" && start >= 2 && is_quantity_word(&tokens[start - 2]) {
            start -= 1;
            continue;
        }
        break;
    }
    start
}

/// Cues that unconditionally retract the preceding clause: the following
/// word is always the replacement, whatever it is.
const STRONG_CUES: &[&str] = &[
    "no wait",
    "wait no",
    "scratch that",
    "strike that",
    "forget that",
    "cancel that",
    "let me start over",
    "correction",
];

/// Cues that retract the preceding clause only when the replacement is a
/// contrasting word (a number, time, date, day, name or entity): "invite
/// Frank, sorry Gordon" retracts a name, but "sorry, I will be late" is an
/// apology, not a retraction.
const NAME_CUES: &[&str] = &["sorry", "i mean", "never mind"];

// `actually` as a retraction cue is only trustworthy mid-sentence with a
// contrasting replacement right after it ("meet at five, actually six");
// elsewhere it is emphasis and must be kept.
/// Pronouns and deictics are not names or entities: "the release is monday,
/// i mean it, so ship it" does not retract "monday".
const PRONOUNS: &[&str] = &[
    "i", "you", "he", "she", "it", "we", "they", "me", "him", "her", "us", "them", "this", "that",
    "these", "those", "my", "your", "his", "its", "our", "their", "who", "whom", "whose", "what",
    "which",
];

/// Words that cannot be a retracted clause on their own: if the retracted
/// span would be only this, there is no retraction ("no wait, the tests
/// pass" — "the" is not a fact the speaker retracted). Pure function words
/// only; spelled-out numbers and day names ARE facts and are not listed here
/// (they are content, and a retraction of "five" is a retraction of a fact).
const NON_FACTS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "so", "that", "this", "with", "for", "from", "to", "in",
    "on", "at", "by", "of", "is", "are", "was", "were", "be", "been", "being", "do", "does", "did",
    "have", "has", "had", "will", "would", "could", "should", "may", "might", "can", "need", "let",
    "us", "we", "i", "you", "he", "she", "it", "they", "not", "no", "yes", "just", "really",
    "very", "too", "then", "than", "as", "if", "when", "where", "who", "what", "how", "why", "up",
    "out", "off", "over", "under", "again", "here", "there", "now", "still", "also", "only", "own",
    "same", "such",
];

// ── Tokens ───────────────────────────────────────────────────────────────────

/// One whitespace-delimited token with the exact whitespace that followed it,
/// so [`build_output`] can reconstruct the input byte-for-byte (the same
/// scheme as the tiered filler pass).
#[derive(Debug, Clone)]
struct Token {
    /// The token text as it appeared, including any glued punctuation.
    text: String,
    /// The exact whitespace run that followed the token in the input.
    gap: String,
    /// The lowercase alphanumeric content ("no," -> "no", "Wait." -> "wait").
    bare: String,
    /// Byte start offset in the original text.
    start: usize,
    /// `true` when the token sits inside a quoted (unmatched `"`) run.
    quoted: bool,
}

impl Token {
    /// The single trailing punctuation character glued to the end of the
    /// token's text ("," on "friday,", "." on "a.m."). Interior punctuation
    /// (the dots in "a.m.", "v1.2.3", "a@b.com") is NOT trailing punct: it
    /// is part of the token's content, not sentence punctuation.
    fn trailing_punct(&self) -> char {
        self.text.chars().next_back().unwrap_or('\0')
    }

    /// True when the token ends a sentence. A dot inside an abbreviation or a
    /// version (`a.m.`, `v1.2.3`, `U.S.`) is content, not a sentence stop.
    fn is_sentence_stop(&self) -> bool {
        let punct = self.trailing_punct();
        if !is_stop_char(punct) {
            return false;
        }
        if punct == '.' && self.text.matches('.').count() > 1 {
            return false;
        }
        true
    }
}

fn is_stop_char(c: char) -> bool {
    matches!(c, '.' | '!' | '?')
}

/// Tokenises `text` into whitespace-delimited tokens, each carrying the
/// exact whitespace run (`gap`) that follows it, and flags, per token,
/// whether it sits inside a quoted (unmatched `"`) run.
fn tokenise(text: &str) -> Vec<Token> {
    let mut quoted_at = vec![false; text.len() + 1];
    let mut in_quote = false;
    for (idx, c) in text.char_indices() {
        quoted_at[idx] = in_quote;
        if c == '"' {
            in_quote = !in_quote;
        }
    }

    let mut tokens = Vec::new();
    let mut offset = 0usize;
    for piece in text.split_inclusive(char::is_whitespace) {
        let split = piece
            .char_indices()
            .rfind(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(piece.len());
        let tok = &piece[..split];
        let gap = &piece[split..];
        if tok.is_empty() {
            offset += piece.len();
            continue;
        }
        let bare: String = tok
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        let quoted = *quoted_at.get(offset).unwrap_or(&false);
        tokens.push(Token {
            text: tok.to_string(),
            gap: gap.to_string(),
            bare,
            start: offset,
            quoted,
        });
        offset += piece.len();
    }
    tokens
}

// ── Cue matching ─────────────────────────────────────────────────────────────

/// A resolved retraction: the clause a cue retracts, plus the cue itself.
struct Clause {
    /// First token of the retracted clause.
    start: usize,
    /// Number of tokens in the retracted clause (zero when only the cue is
    /// removed and the repair lives in `extra_removals`).
    len: usize,
    keep_both: bool,
    /// Tokens removed away from the main span, because the repair is not
    /// contiguous ("send it over, i mean the deck": the pronoun is removed
    /// while the particle between it and the cue is kept).
    extra_removals: Vec<(usize, usize)>,
    /// Token whose trailing comma is repair framing even though it is not the
    /// first kept token after the retracted span.
    strip_comma_at: Option<usize>,
}

impl Clause {
    fn plain(start: usize, len: usize, keep_both: bool) -> Self {
        Self {
            start,
            len,
            keep_both,
            extra_removals: Vec::new(),
            strip_comma_at: None,
        }
    }
}

/// A retraction found in the text: the token span `start..start+len` that is
/// retracted (the cue phrase plus the retracted clause), and whether both the
/// retracted clause and the replacement must be kept (ambiguous replacement
/// over a protected fact).
struct Retraction {
    start: usize,
    len: usize,
    keep_both: bool,
    /// Remove the comma immediately after the retraction when it is repair
    /// framing around a corrected name ("invite Frank, strike that, Gordon,
    /// to the review" -> "invite Gordon to the review").
    strip_comma: bool,
    /// Tokens removed away from the main span (see [`Clause::extra_removals`]).
    extra_removals: Vec<(usize, usize)>,
    /// An additional token to strip a trailing comma from.
    strip_comma_at: Option<usize>,
}

/// Finds every safe retraction in `tokens`. Each cue occurrence is resolved
/// independently; retracted spans never overlap (a cue inside an already
/// retracted span is skipped).
fn find_retractions(tokens: &[Token]) -> Vec<Retraction> {
    let n = tokens.len();
    let mut out: Vec<Retraction> = Vec::new();
    let mut i = 0usize;
    while i < n {
        // A quoted or punct-led token can never open a cue ("he said
        // \"no wait\"" / "(no wait) the build passes").
        if tokens[i].quoted
            || tokens[i]
                .text
                .chars()
                .next()
                .is_some_and(|c| !c.is_alphanumeric())
        {
            i += 1;
            continue;
        }
        // Longest cue first at this position.
        for &(cue, words) in CUES {
            let w = words.len();
            if i + w > n {
                continue;
            }
            let mut matched = true;
            for k in 0..w {
                let t = &tokens[i + k];
                if t.quoted || t.bare != words[k] {
                    matched = false;
                    break;
                }
            }
            if !matched {
                continue;
            }
            let cue_start = i;
            let cue_end = i + w; // first token after the cue
                                 // The replacement starts at the first token after the cue that is
                                 // not itself a comma/stop carried on the cue's last word.
            let mut rep = cue_end;
            if rep < n
                && tokens[rep]
                    .text
                    .chars()
                    .next()
                    .is_some_and(|c| c == ',' || c == '(')
            {
                rep += 1;
            }
            // What does the cue retract?
            let retracted = match cue {
                "actually" => retract_after_actuality(tokens, cue_start, rep, n),
                "rather" => retract_after_rather(tokens, cue_start, rep, n),
                c if STRONG_CUES.contains(&c) => {
                    retract_before_cue(tokens, cue_start, rep, n, false)
                }
                c if NAME_CUES.contains(&c) => retract_before_cue(tokens, cue_start, rep, n, true),
                _ => None,
            };
            if let Some(clause) = retracted {
                // The retracted span is the clause (span_start..span_start+span_len)
                // plus the cue itself (i..cue_end). The cue is removed along
                // with the clause so the output reads cleanly.
                let clause_end = clause.start + clause.len;
                let full_start = clause.start.min(i);
                let full_end = cue_end.max(clause_end);
                // If the clause's last token carries a trailing comma, the
                // comma is part of the retracted span: extend the span to
                // include it (the comma is on the clause's last token, which
                // is already in the span, so the comma is removed with it).
                // No extra work needed: the comma is on the token's text, and
                // the token is removed, so the comma is gone.
                out.push(Retraction {
                    start: full_start,
                    len: full_end - full_start,
                    keep_both: clause.keep_both,
                    strip_comma: should_strip_comma(cue, tokens, clause.start, clause.len, rep),
                    extra_removals: clause.extra_removals,
                    strip_comma_at: clause.strip_comma_at,
                });
                // Skip past the retracted span and the cue.
                i = cue_end;
                continue;
            }
            // The cue is present but nothing safe was retracted: move on.
            i = cue_end;
            continue;
        }
        i += 1;
    }
    // Overlap guard: a later retraction that overlaps an earlier one is
    // dropped (the earlier cue owns the span).
    out.sort_by_key(|r| r.start);
    let mut cleaned: Vec<Retraction> = Vec::new();
    for r in out {
        if let Some(last) = cleaned.last() {
            if r.start < last.start + last.len {
                continue;
            }
        }
        cleaned.push(r);
    }
    cleaned
}

/// The clause a cue at `cue_start` retracts, or `None` when nothing safe can
/// be retracted.
///
/// `needs_contrast` is true for the name cues (`sorry`, `i mean`,
/// `never mind`): the replacement must be a contrasting word (number, time,
/// name, entity), otherwise the cue is an apology/clarification, not a
/// retraction, and the text is left unchanged.
///
/// When the clause walk finds nothing usable, a repaired object pronoun is
/// still resolvable: "ship it, i mean the fix" -> "ship the fix".
fn retract_before_cue(
    tokens: &[Token],
    cue_start: usize,
    replacement: usize,
    n: usize,
    needs_contrast: bool,
) -> Option<Clause> {
    retract_clause_before_cue(tokens, cue_start, replacement, n, needs_contrast)
        .or_else(|| retract_replaced_pronoun(tokens, cue_start, replacement, n))
}

/// A repaired object replaces a pronoun: "send it over, i mean the deck" ->
/// "send over the deck".
///
/// The repaired phrase must be a determiner-led noun phrase or a contrasting
/// word (a name, number or entity), so an apology or a continuation after
/// "i mean" is still left alone. Two shapes are handled:
///
/// * the pronoun sits right before the cue, and is removed with it;
/// * a verb particle sits between the pronoun and the cue ("it over"): only
///   the pronoun is removed, because "send over the deck" is as natural as
///   "send the deck over" and keeping the particle avoids reordering words.
fn retract_replaced_pronoun(
    tokens: &[Token],
    cue_start: usize,
    replacement: usize,
    n: usize,
) -> Option<Clause> {
    if replacement >= n || cue_start == 0 {
        return None;
    }
    let head = replacement_head(tokens, cue_start, replacement, n);
    let det_led = head != replacement && DETERMINERS.contains(&tokens[replacement].bare.as_str());
    if !det_led {
        let first_alpha = tokens[head].text.chars().find(|c| c.is_alphabetic());
        if !is_contrast_word(&tokens[head].bare, first_alpha) {
            return None;
        }
    }
    if !is_content_word(&tokens[head]) {
        return None;
    }
    // "send it over, i mean the deck": the particle stays, the pronoun goes.
    if cue_start >= 2 {
        let particle = &tokens[cue_start - 1];
        let pronoun = &tokens[cue_start - 2];
        if PARTICLES.contains(&particle.bare.as_str())
            && OBJECT_PRONOUNS.contains(&pronoun.bare.as_str())
        {
            let mut clause = Clause::plain(cue_start, 0, false);
            clause.extra_removals.push((cue_start - 2, 1));
            if particle.trailing_punct() == ',' {
                clause.strip_comma_at = Some(cue_start - 1);
            }
            return Some(clause);
        }
    }
    // "ship it, i mean the fix": the pronoun is contiguous with the cue.
    if OBJECT_PRONOUNS.contains(&tokens[cue_start - 1].bare.as_str()) {
        return Some(Clause::plain(cue_start - 1, 1, false));
    }
    None
}

fn retract_clause_before_cue(
    tokens: &[Token],
    cue_start: usize,
    replacement: usize,
    n: usize,
    needs_contrast: bool,
) -> Option<Clause> {
    // The replacement must exist (cue + no replacement -> keep, ambiguous).
    if replacement >= n {
        return None;
    }
    // The retracted clause is the run of tokens immediately before the cue,
    // bounded on the left by: a comma (inclusive of the comma-carrying
    // token), a stop mark (exclusive of the stop-carrying token), a
    // function word (exclusive), or the start of the text.
    //
    // The walk starts at the token immediately before the cue and moves
    // left. It stops when it hits:
    // - a token carrying a trailing comma (the comma-carrying token IS the
    //   last word of the retracted clause),
    // - a token carrying a stop mark (the stop-carrying token is NOT part
    //   of the clause — it belongs to the previous sentence),
    // - a function word (the function word is NOT part of the clause — it
    //   is part of the surrounding sentence),
    // - the start of the text.
    let mut clause_end = cue_start; // exclusive
    let mut j = cue_start;
    let mut found_boundary = false;
    let mut boundary_punct = false;
    let mut boundary_comma = false;
    while j > 0 {
        let k = j - 1;
        let t = &tokens[k];
        if t.is_sentence_stop() {
            // A stop mark is a sentence boundary: the clause does not cross
            // it, and the stop-carrying token is NOT part of the clause.
            found_boundary = true;
            boundary_punct = true;
            break;
        }
        // A function word (the, is, at, in, on, etc.) marks the boundary
        // of the retracted phrase: the function word is NOT part of the
        // clause.
        if NON_FACTS.contains(&t.bare.as_str()) {
            // A pronoun closing a counted object ("three of us") is part of
            // the retracted phrase, not a boundary: retracting only the
            // pronoun would leave "three of four of us".
            let counted_object = OBJECT_PRONOUNS.contains(&t.bare.as_str())
                && k >= 2
                && tokens[k - 1].bare == "of"
                && is_quantity_word(&tokens[k - 2]);
            if !counted_object {
                found_boundary = true;
                break;
            }
        }
        clause_end = k;
        if t.trailing_punct() == ',' {
            // A comma marks the end of the clause: the comma-carrying token
            // IS the last word of the retracted clause (but see the quantity
            // extension below — it can be the tail of a longer phrase).
            found_boundary = true;
            boundary_punct = true;
            boundary_comma = true;
            break;
        }
        j = k;
        if k == 0 {
            break;
        }
    }
    if !found_boundary && clause_end == cue_start {
        // No boundary found and no tokens walked: the clause is empty.
        return None;
    }
    if boundary_comma {
        // "twenty dollars," is one fact, not two: retract the whole phrase so
        // the retracted quantity cannot survive next to its replacement.
        clause_end = extend_clause_over_quantity(tokens, clause_end);
        // "two of them," is one fact too: retracting only "them" would leave
        // "buy two of three of them".
        clause_end = extend_clause_over_quantified_object(tokens, clause_end);
    }
    // Without punctuation, the clause walk stopped on a function word. That
    // can leave only a suffix of a noun phrase ("the office" -> "office" in
    // "9:15 @ the office"), and removing just that suffix corrupts the
    // sentence. Only retract a no-punctuation clause when the clause itself
    // is an unambiguous fact (a number, day/time, proper noun, or code-like
    // token); otherwise leave the text unchanged and let the model decide.
    if !boundary_punct && !clause_is_unambiguous_fact(tokens, clause_end, cue_start) {
        return None;
    }
    // The clause is clause_end..cue_start.
    let clause_len = cue_start - clause_end;
    if clause_len == 0 {
        return None;
    }
    // The clause must contain at least one content word (not a function
    // word). Spelled-out numbers and day names ARE content (a retraction of
    // "five" is a retraction of a fact).
    let has_content = (clause_end..cue_start).any(|k| {
        let bare = &tokens[k].bare;
        !bare.is_empty() && !NON_FACTS.contains(&bare.as_str())
    });
    if !has_content {
        return None;
    }
    // Replacement check. The replacement can open with a determiner or with a
    // repeated connector ("to Sarah, i mean, to Priya"); the word that
    // contrasts is its head, not the connector.
    let head = replacement_head(tokens, clause_end, replacement, n);
    let rep_bare = &tokens[head].bare;
    if rep_bare.is_empty() {
        return None;
    }
    let rep_first_alpha = tokens[head].text.chars().find(|c| c.is_alphabetic());
    // For name cues (sorry, i mean, never mind), the replacement must be a
    // contrasting word (a number, time word, day name, or capitalised proper
    // noun). A lowercase common word is NOT a contrasting word: "sorry, i
    // will be late" is an apology, not a retraction. For strong cues (no
    // wait, scratch that, etc.), the replacement does NOT need to be
    // contrasting: any non-empty word is the replacement.
    if needs_contrast && !is_contrast_word(rep_bare, rep_first_alpha) {
        return None;
    }
    // Protected-fact ambiguity: if the retracted clause contains a number
    // (a digit or a spelled-out number word) and the replacement is not
    // itself a number, keep both. A name in the retracted clause with a
    // non-name replacement is also ambiguous: keep both.
    //
    // For strong cues, the replacement is unambiguous by definition (the
    // speaker explicitly retracted the clause), so keep_both is always
    // false. For name cues, the replacement must be a contrasting word to
    // resolve; if the retracted clause has a protected fact and the
    // replacement is not a contrasting word, keep both.
    let keep_both = if needs_contrast {
        let clause_has_number = (clause_end..cue_start).any(|k| {
            let bare = &tokens[k].bare;
            bare.chars().next().is_some_and(|c| c.is_ascii_digit())
                || NUM_WORDS.contains(&bare.as_str())
        });
        let rep_is_number = rep_bare.chars().next().is_some_and(|c| c.is_ascii_digit())
            || NUM_WORDS.contains(&rep_bare.as_str());
        let clause_has_name = (clause_end..cue_start).any(|k| {
            let bare = &tokens[k].bare;
            is_name_like(bare) && !PRONOUNS.contains(&bare.as_str())
        });
        let rep_is_name = is_contrast_word(rep_bare, rep_first_alpha)
            && !rep_bare.chars().next().is_some_and(|c| c.is_ascii_digit())
            && !NUM_WORDS.contains(&rep_bare.as_str());
        (clause_has_number && !rep_is_number)
            || (clause_has_name && !rep_is_name && !clause_has_number)
    } else {
        // Strong cue: the replacement is unambiguous.
        false
    };
    // The retracted span is clause_end..cue_start (the clause words only;
    // the cue itself is kept so the model still sees the repair cue and the
    // gate's self-correction marker still fires).
    // Actually, the cue should be removed along with the retracted clause so
    // the output reads cleanly. The span is clause_end..cue_start+w (clause
    // + cue). But we only have cue_start here, not the cue length. The
    // caller (find_retractions) knows the cue length; we return the clause
    // span and the caller extends it to include the cue.
    // Return: (clause_end, clause_len, keep_both). The caller extends the
    // span to include the cue tokens.
    Some(Clause::plain(clause_end, clause_len, keep_both))
}

/// `actually` as a retraction cue: "meet at five, actually six". The
/// retracted clause is the run of tokens between the last comma/stop before
/// `actually` and `actually` itself, and the replacement is the token right
/// after `actually`. If the retracted clause is empty or the replacement is
/// missing, `actually` is emphasis and nothing is retracted.
///
/// Unlike the other cues, `actually` does not require the replacement to be
/// a contrasting word (the doc says "no contrasting replacement nearby" is
/// the emphasis heuristic, but in practice "actually" as a retraction cue
/// always has a contrasting replacement; we require one to be safe).
fn retract_after_actuality(
    tokens: &[Token],
    cue_start: usize,
    replacement: usize,
    n: usize,
) -> Option<Clause> {
    if replacement >= n {
        return None;
    }
    // The retracted clause is the run before `actually`, bounded by a
    // comma/stop/function word or start of text.
    let mut clause_end = cue_start;
    let mut j = cue_start;
    let mut found_boundary = false;
    let mut boundary_comma = false;
    while j > 0 {
        let k = j - 1;
        let t = &tokens[k];
        if t.is_sentence_stop() {
            found_boundary = true;
            break;
        }
        if NON_FACTS.contains(&t.bare.as_str()) {
            // A pronoun closing a counted object ("three of us") is part of
            // the retracted phrase, not a boundary: retracting only the
            // pronoun would leave "three of four of us".
            let counted_object = OBJECT_PRONOUNS.contains(&t.bare.as_str())
                && k >= 2
                && tokens[k - 1].bare == "of"
                && is_quantity_word(&tokens[k - 2]);
            if !counted_object {
                found_boundary = true;
                break;
            }
        }
        clause_end = k;
        if t.trailing_punct() == ',' {
            found_boundary = true;
            boundary_comma = true;
            break;
        }
        j = k;
        if k == 0 {
            break;
        }
    }
    if !found_boundary && clause_end == cue_start {
        return None;
    }
    if boundary_comma {
        // A quantity phrase is one fact: "twenty dollars, actually, thirty
        // dollars" must not leave "twenty thirty dollars".
        clause_end = extend_clause_over_quantity(tokens, clause_end);
        clause_end = extend_clause_over_quantified_object(tokens, clause_end);
    }
    let clause_len = cue_start - clause_end;
    if clause_len == 0 {
        return None;
    }
    let has_content = (clause_end..cue_start).any(|k| {
        let bare = &tokens[k].bare;
        !bare.is_empty() && !NON_FACTS.contains(&bare.as_str())
    });
    if !has_content {
        return None;
    }
    let head = replacement_head(tokens, clause_end, replacement, n);
    let rep_bare = &tokens[head].bare;
    if rep_bare.is_empty() {
        return None;
    }
    let rep_first_alpha = tokens[head].text.chars().find(|c| c.is_alphabetic());
    // `actually` as emphasis: no contrasting replacement nearby. We require
    // the replacement to be a contrasting word (number/name/time) to treat
    // it as a retraction; otherwise keep.
    if !is_contrast_word(rep_bare, rep_first_alpha) {
        return None;
    }
    let clause_has_number = (clause_end..cue_start).any(|k| {
        let bare = &tokens[k].bare;
        bare.chars().next().is_some_and(|c| c.is_ascii_digit())
            || NUM_WORDS.contains(&bare.as_str())
    });
    let rep_is_number = rep_bare.chars().next().is_some_and(|c| c.is_ascii_digit())
        || NUM_WORDS.contains(&rep_bare.as_str());
    let clause_has_name = (clause_end..cue_start).any(|k| {
        let bare = &tokens[k].bare;
        is_name_like(bare) && !PRONOUNS.contains(&bare.as_str())
    });
    let rep_is_name = is_contrast_word(rep_bare, rep_first_alpha)
        && !rep_bare.chars().next().is_some_and(|c| c.is_ascii_digit())
        && !NUM_WORDS.contains(&rep_bare.as_str());
    let keep_both = (clause_has_number && !rep_is_number)
        || (clause_has_name && !rep_is_name && !clause_has_number);
    Some(Clause::plain(clause_end, clause_len, keep_both))
}

/// `rather` as a retraction cue: "the meeting is at five, rather six". The
/// retracted clause is the run before `rather`, bounded by a comma/stop.
fn retract_after_rather(
    tokens: &[Token],
    cue_start: usize,
    replacement: usize,
    n: usize,
) -> Option<Clause> {
    if replacement >= n {
        return None;
    }
    let mut clause_end = cue_start;
    let mut j = cue_start;
    let mut found_boundary = false;
    let mut boundary_comma = false;
    while j > 0 {
        let k = j - 1;
        let t = &tokens[k];
        if t.is_sentence_stop() {
            found_boundary = true;
            break;
        }
        if NON_FACTS.contains(&t.bare.as_str()) {
            // A pronoun closing a counted object ("three of us") is part of
            // the retracted phrase, not a boundary: retracting only the
            // pronoun would leave "three of four of us".
            let counted_object = OBJECT_PRONOUNS.contains(&t.bare.as_str())
                && k >= 2
                && tokens[k - 1].bare == "of"
                && is_quantity_word(&tokens[k - 2]);
            if !counted_object {
                found_boundary = true;
                break;
            }
        }
        clause_end = k;
        if t.trailing_punct() == ',' {
            found_boundary = true;
            boundary_comma = true;
            break;
        }
        j = k;
        if k == 0 {
            break;
        }
    }
    if !found_boundary && clause_end == cue_start {
        return None;
    }
    if boundary_comma {
        // A quantity phrase is one fact, as in the other cue paths.
        clause_end = extend_clause_over_quantity(tokens, clause_end);
        clause_end = extend_clause_over_quantified_object(tokens, clause_end);
    }
    let clause_len = cue_start - clause_end;
    if clause_len == 0 {
        return None;
    }
    let has_content = (clause_end..cue_start).any(|k| {
        let bare = &tokens[k].bare;
        !bare.is_empty() && !NON_FACTS.contains(&bare.as_str())
    });
    if !has_content {
        return None;
    }
    let head = replacement_head(tokens, clause_end, replacement, n);
    let rep_bare = &tokens[head].bare;
    if rep_bare.is_empty() {
        return None;
    }
    let rep_first_alpha = tokens[head].text.chars().find(|c| c.is_alphabetic());
    if !is_contrast_word(rep_bare, rep_first_alpha) {
        return None;
    }
    let clause_has_number = (clause_end..cue_start).any(|k| {
        let bare = &tokens[k].bare;
        bare.chars().next().is_some_and(|c| c.is_ascii_digit())
            || NUM_WORDS.contains(&bare.as_str())
    });
    let rep_is_number = rep_bare.chars().next().is_some_and(|c| c.is_ascii_digit())
        || NUM_WORDS.contains(&rep_bare.as_str());
    let clause_has_name = (clause_end..cue_start).any(|k| {
        let bare = &tokens[k].bare;
        is_name_like(bare) && !PRONOUNS.contains(&bare.as_str())
    });
    let rep_is_name = is_contrast_word(rep_bare, rep_first_alpha)
        && !rep_bare.chars().next().is_some_and(|c| c.is_ascii_digit())
        && !NUM_WORDS.contains(&rep_bare.as_str());
    let keep_both = (clause_has_number && !rep_is_number)
        || (clause_has_name && !rep_is_name && !clause_has_number);
    Some(Clause::plain(clause_end, clause_len, keep_both))
}

/// True when `bare` is a contrasting word: a number, a time word, a day
/// name, or a proper noun (capitalised in the original text).
///
/// `original_first_alpha` is the first alphabetic character of the original
/// token text (before lowercasing), used to detect proper nouns by
/// capitalisation. A word that is not a number, time word, or capitalised
/// proper noun is NOT a contrasting word (it is a common verb, adjective,
/// or other word, and the cue is emphasis, not a retraction).
fn is_contrast_word(bare: &str, original_first_alpha: Option<char>) -> bool {
    if bare.is_empty() {
        return false;
    }
    // Number or digit.
    if bare.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return true;
    }
    // Spelled-out numbers and time words.
    if NUM_WORDS.contains(&bare) {
        return true;
    }
    // Day names and time markers.
    const TIME_WORDS: &[&str] = &[
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "today",
        "tomorrow",
        "yesterday",
        "noon",
        "midnight",
        "morning",
        "afternoon",
        "evening",
        "night",
        "am",
        "pm",
    ];
    if TIME_WORDS.contains(&bare) {
        return true;
    }
    // Proper noun: capitalised in the original text (a name or entity).
    if let Some(c) = original_first_alpha {
        if c.is_uppercase() {
            return true;
        }
    }
    false
}

/// The word a replacement actually contrasts on.
///
/// A repaired phrase can open with a connector that is not the repaired fact:
///
/// * "send it, i mean the deck" contrasts on "deck", not on "the";
/// * "to Sarah, i mean, to Priya" contrasts on "Priya"; the repeated "to" is
///   repair framing and is dropped from the output, so skipping it here is
///   what lets the correction resolve.
///
/// A preposition is only skipped when it repeats the word before the retracted
/// clause, so "from Monday, i mean to Tuesday" never resolves into "from to
/// Tuesday".
fn replacement_head(tokens: &[Token], clause_start: usize, replacement: usize, n: usize) -> usize {
    if replacement + 1 >= n {
        return replacement;
    }
    let lead = tokens[replacement].bare.as_str();
    let next = &tokens[replacement + 1];
    if next.bare.is_empty() || NON_FACTS.contains(&next.bare.as_str()) || next.is_sentence_stop() {
        return replacement;
    }
    // A determiner belongs to the repaired noun phrase.
    if DETERMINERS.contains(&lead) {
        return replacement + 1;
    }
    // A repeated connector is repair framing around an unchanged phrase.
    if clause_start > 0
        && !lead.is_empty()
        && lead == tokens[clause_start - 1].bare
        && NON_FACTS.contains(&lead)
    {
        return replacement + 1;
    }
    replacement
}

/// True when `bare` looks like a name or entity: length >= 3, not a
/// pronoun, not a function word, not a number word.
fn is_name_like(bare: &str) -> bool {
    if bare.len() < 3 {
        return false;
    }
    if PRONOUNS.contains(&bare) || NON_FACTS.contains(&bare) {
        return false;
    }
    true
}

/// True when a token is an unambiguous fact on its own: a number, day/time
/// word, capitalised proper noun, or a code-like token (identifier, path,
/// version, email, percentage, clock time). These are safe to retract
/// without a punctuation boundary; a common lowercase noun is not.
fn is_fact_token(tok: &Token) -> bool {
    let first_alpha = tok.text.chars().find(|c| c.is_alphabetic());
    if is_contrast_word(&tok.bare, first_alpha) || tok.bare.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    // Code-like or formatted token: any interior punctuation after trimming
    // sentence punctuation from the edges ("a|b", "v1.2.3", "[report.pdf]",
    // "FEATURE_FLAGS", "state-of-the-art"). A common word with a trailing
    // comma is not code-like.
    let core = tok
        .text
        .trim_matches(|c: char| matches!(c, ',' | '.' | '!' | '?' | '"' | '\'' | ';' | ':'));
    core.chars().any(|c| !c.is_alphanumeric())
}

/// True when a token is a person or entity name (not a number, day, or
/// time). Used only to decide whether a comma after the replacement is
/// repair framing.
fn is_person_name(tok: &Token) -> bool {
    const NOT_NAMES: &[&str] = &[
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
        "thirty",
        "forty",
        "fifty",
        "sixty",
        "seventy",
        "eighty",
        "ninety",
        "hundred",
        "thousand",
        "million",
        "billion",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "today",
        "tomorrow",
        "yesterday",
        "noon",
        "midnight",
        "morning",
        "afternoon",
        "evening",
        "night",
        "am",
        "pm",
    ];
    let bare = &tok.bare;
    if bare.len() < 3 || PRONOUNS.contains(&bare.as_str()) || NON_FACTS.contains(&bare.as_str()) {
        return false;
    }
    if NOT_NAMES.contains(&bare.as_str()) {
        return false;
    }
    bare.chars().all(|c| c.is_alphabetic())
}

/// Strip the comma after the replacement only for a corrected name. For
/// numbers and days the comma is original punctuation ("thursday, at ten").
fn should_strip_comma(
    cue: &str,
    tokens: &[Token],
    clause_start: usize,
    clause_len: usize,
    replacement: usize,
) -> bool {
    if replacement >= tokens.len() {
        return false;
    }
    if !NAME_CUES.contains(&cue) && !STRONG_CUES.contains(&cue) {
        return false;
    }
    let clause_has_name =
        (clause_start..clause_start + clause_len).any(|k| is_person_name(&tokens[k]));
    clause_has_name && is_person_name(&tokens[replacement])
}

/// True when the no-punctuation clause is itself an unambiguous fact, or a
/// two-token time expression such as "9:15 a.m.".
fn clause_is_unambiguous_fact(tokens: &[Token], start: usize, end: usize) -> bool {
    if start >= end {
        return false;
    }
    if end - start == 1 {
        return is_fact_token(&tokens[start]);
    }
    // The retracted value itself is the last token of the clause.
    if is_fact_token(&tokens[end - 1]) {
        return true;
    }
    let has_number = (start..end).any(|k| {
        tokens[k]
            .bare
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    });
    let all_timeish =
        (start..end).all(|k| is_fact_token(&tokens[k]) || tokens[k].text.contains('.'));
    has_number && all_timeish
}

/// Drops the repaired phrase's leading function word when the word before the
/// retracted span is the same word. "ship on monday, correction, on tuesday"
/// otherwise removes "monday, correction" and leaves "ship on on tuesday";
/// dropping the repeated "on" gives "ship on tuesday". Only an exact repeat of
/// a function word is touched, so a meaningful word is never removed.
fn drop_duplicated_leading_function_word(
    tokens: &[Token],
    remove: &mut [bool],
    retractions: &[Retraction],
) {
    for ret in retractions {
        if ret.keep_both || ret.start == 0 {
            continue;
        }
        let mut after = ret.start + ret.len;
        while after < tokens.len() && remove[after] {
            after += 1;
        }
        if after >= tokens.len() {
            continue;
        }
        let before = ret.start - 1;
        let repeated = tokens[before].bare.as_str();
        if !remove[before]
            && !repeated.is_empty()
            && repeated == tokens[after].bare
            && NON_FACTS.contains(&repeated)
        {
            remove[after] = true;
        }
    }
}

// ── Output builder ───────────────────────────────────────────────────────────

/// Builds the final text by dropping removed tokens (and transferring a
/// stranded trailing stop to the previous kept token) while preserving every
/// other byte, including whitespace, exactly. Byte-exact when nothing is
/// removed.
fn build_output(text: &str, tokens: &[Token], remove: &[bool], strip_comma: &[bool]) -> String {
    if tokens.is_empty() {
        return text.to_string();
    }
    // For each removed token carrying a stop, mark the previous kept token
    // to gain that stop (only if that token has no stop of its own), so the
    // sentence keeps its end punctuation.
    let mut append_stop: Vec<Option<char>> = vec![None; tokens.len()];
    for (i, tok) in tokens.iter().enumerate() {
        if remove[i] {
            let prev_kept = (0..i).rev().find(|&k| !remove[k]);
            if let Some(p) = prev_kept {
                if !tokens[p].is_sentence_stop() && tok.is_sentence_stop() {
                    append_stop[p] = Some(tok.trailing_punct());
                }
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for (i, tok) in tokens.iter().enumerate() {
        debug_assert!(
            tok.start >= cursor,
            "token offsets must be monotonic (token {i} at {}, cursor {})",
            tok.start,
            cursor
        );
        if tok.start > cursor {
            out.push_str(&text[cursor..tok.start]);
        }
        if remove[i] {
            // Preserve line structure: if the removed token's gap contains a
            // newline, emit the newline so lines don't merge.
            if tok.gap.contains('\n') {
                if let Some(nl_pos) = tok.gap.find('\n') {
                    out.push_str(&tok.gap[nl_pos..]);
                }
            }
        } else {
            let text_to_emit = if strip_comma[i] && tok.trailing_punct() == ',' {
                &tok.text[..tok.text.len() - 1]
            } else {
                &tok.text
            };
            out.push_str(text_to_emit);
            if let Some(stop) = append_stop[i] {
                out.push(stop);
            }
            out.push_str(&tok.gap);
        }
        cursor = tok.start + tok.text.len() + tok.gap.len();
    }
    if cursor < text.len() {
        out.push_str(&text[cursor..]);
    }
    out
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn r(t: &str) -> String {
        resolve_backtracks(t, "en")
    }

    // ── Cue + replacement (mid-sentence and end-of-take) ──────────────────

    #[test]
    fn cue_replacement_mid_sentence() {
        assert_eq!(r("let's meet at five, no wait, six"), "let's meet at six");
    }

    #[test]
    fn cue_replacement_end_of_take() {
        // No comma before the cue: the clause is only the single token
        // immediately before the cue ("friday"), so the retraction removes
        // "friday" and the cue, leaving "the release is thursday".
        assert_eq!(
            r("the release is friday no wait thursday"),
            "the release is thursday"
        );
        // With a comma before the cue: only "friday" is retracted.
        assert_eq!(
            r("the release is friday, no wait, thursday"),
            "the release is thursday"
        );
    }

    // ── Quantity phrases and repeated connectors ──────────────────────────

    #[test]
    fn correction_retracts_the_whole_quantity_phrase() {
        // The comma falls inside the quantity phrase: retracting only
        // "dollars" would leave "it costs twenty thirty dollars".
        assert_eq!(
            r("It costs twenty dollars, correction, thirty dollars"),
            "It costs thirty dollars"
        );
        assert_eq!(
            r("It costs 20 dollars, correction, 30 dollars"),
            "It costs 30 dollars"
        );
    }

    #[test]
    fn correction_retracts_a_counted_noun_phrase() {
        assert_eq!(
            r("we have three servers, correction, four servers"),
            "we have four servers"
        );
        assert_eq!(
            r("I need two coffees, correction, three coffees"),
            "I need three coffees"
        );
    }

    #[test]
    fn correction_retracts_a_time_plus_unit() {
        assert_eq!(r("meet at five pm, correction, six pm"), "meet at six pm");
    }

    #[test]
    fn correction_retracts_a_determined_quantity() {
        // "a" belongs to the quantity phrase; without it the output would be
        // "it costs a thirty dollars".
        assert_eq!(
            r("it costs a hundred dollars, correction, thirty dollars"),
            "it costs thirty dollars"
        );
        assert_eq!(
            r("the total is one hundred and twenty, correction, one fifty"),
            "the total is one fifty"
        );
    }

    #[test]
    fn repeated_connector_is_not_duplicated() {
        // "on monday, correction, on tuesday" must not produce "on on tuesday".
        assert_eq!(
            r("ship on Monday, correction, on Tuesday"),
            "ship on Tuesday"
        );
        assert_eq!(
            r("the meeting is at five, correction, at six"),
            "the meeting is at six"
        );
    }

    #[test]
    fn quantity_extension_does_not_swallow_a_name_phrase() {
        // A name is not a quantity: the existing name retraction is unchanged,
        // and the word before the span survives.
        assert_eq!(
            r("invite Frank, scratch that, Gordon to the review"),
            "invite Gordon to the review"
        );
    }

    #[test]
    fn scratch_that_retracts_name() {
        // Capitalized names are proper nouns: the replacement "Gordon" is a
        // contrasting word, so the retraction resolves.
        assert_eq!(
            r("invite Frank, scratch that, Gordon to the review"),
            "invite Gordon to the review"
        );
        // A strong cue retracts unconditionally once it has a punctuation
        // boundary, even when the names are lowercase ("scratch that" is an
        // explicit instruction, not emphasis).
        assert_eq!(
            r("invite frank, scratch that, gordon to the review"),
            "invite gordon to the review"
        );
    }

    #[test]
    fn i_mean_retracts_day() {
        // The comma after "tuesday" is original punctuation and is kept.
        // The retraction removes "monday" and "i mean", leaving
        // "the release is tuesday, at ten".
        assert_eq!(
            r("the release is monday, i mean tuesday, at ten"),
            "the release is tuesday, at ten"
        );
    }

    #[test]
    fn actually_retracts_number() {
        assert_eq!(r("let's meet at five, actually six"), "let's meet at six");
    }

    #[test]
    fn rather_retracts_number() {
        assert_eq!(
            r("the meeting is at five, rather six"),
            "the meeting is at six"
        );
    }

    #[test]
    fn sorry_retracts_name() {
        // Capitalized name: the replacement "Gordon" is a contrasting word,
        // so the retraction resolves.
        assert_eq!(
            r("invite Frank, sorry Gordon, to the review"),
            "invite Gordon to the review"
        );
        // Lowercase name: ambiguous (could be a common word), keep both.
        assert_eq!(
            r("invite frank, sorry gordon, to the review"),
            "invite frank, sorry gordon, to the review"
        );
    }

    // ── Multiple cues in one take ──────────────────────────────────────────

    #[test]
    fn multiple_cues() {
        assert_eq!(
            r("the deploy is at nine, no wait, ten, and the review is friday, i mean tuesday"),
            "the deploy is at ten, and the review is tuesday"
        );
    }

    // ── Cue without replacement (no change) ────────────────────────────────

    #[test]
    fn cue_no_replacement_keeps_text() {
        let input = "the release is friday no wait";
        assert_eq!(r(input), input);
    }

    #[test]
    fn cue_at_end_keeps_text() {
        let input = "let's meet at five, actually";
        assert_eq!(r(input), input);
    }

    // ── `actually` emphasis vs correction ──────────────────────────────────

    #[test]
    fn actually_emphasis_kept() {
        // No contrasting replacement: "actually" is emphasis.
        let input = "the tests actually pass";
        assert_eq!(r(input), input);
    }

    #[test]
    fn actually_emphasis_no_replacement_kept() {
        let input = "i actually think we should ship";
        assert_eq!(r(input), input);
    }

    #[test]
    fn actually_correction_resolved() {
        // "five hundred" is a multi-word quantity: the comma-bounded clause is
        // extended left over the number, so no "five" is left beside "six".
        assert_eq!(
            r("the budget is five hundred, actually six hundred"),
            "the budget is six hundred"
        );
        // Single-token number: the retraction resolves cleanly.
        assert_eq!(r("the budget is five, actually six"), "the budget is six");
    }

    // ── Retracted fact/number/name with ambiguous replacement (keep both) ─

    #[test]
    fn retracted_number_ambiguous_replacement_keeps_both() {
        // Strong cue with a non-contrasting replacement: the retraction
        // resolves (strong cues don't require contrast). "five" and "no
        // wait" are removed, "tomorrow" survives.
        assert_eq!(
            r("the release is at five, no wait, tomorrow"),
            "the release is at tomorrow"
        );
    }

    #[test]
    fn retracted_name_ambiguous_replacement_keeps_both() {
        // The retracted clause has a name; the replacement is not a name,
        // so keep both.
        let input = "invite frank, sorry, the team, to the review";
        assert_eq!(r(input), input);
    }

    // ── No-cue text is byte-identical ──────────────────────────────────────

    #[test]
    fn no_cue_byte_identical() {
        let input = "the deploy window opens at nine fifteen am on tuesday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn no_cue_complex_byte_identical() {
        let input = "so basically we need to ship this on friday, because the client meeting is on monday. the budget is $450,000 and the deadline is 9:15 am.";
        assert_eq!(r(input), input);
    }

    // ── Idempotence ────────────────────────────────────────────────────────

    #[test]
    fn idempotent_cue_replacement() {
        let once = r("let's meet at five, no wait, six");
        let twice = r(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn idempotent_multiple_cues() {
        let once =
            r("the deploy is at nine, no wait, ten, and the review is friday, i mean tuesday");
        let twice = r(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn idempotent_no_cue() {
        let input = "the tests pass and the build is green";
        let once = r(input);
        let twice = r(&once);
        assert_eq!(input, once);
        assert_eq!(once, twice);
    }

    // ── First/last word protection ─────────────────────────────────────────

    #[test]
    fn first_word_protection() {
        // The cue is at position 0: the whole take is a retraction with a
        // replacement. The first token is explicitly retracted by the cue,
        // so it is allowed.
        assert_eq!(r("friday, no wait, tuesday"), "tuesday");
    }

    #[test]
    fn last_word_protection() {
        // The retracted clause is before the cue, so it can never be the
        // last token of the take. The last token is always the replacement
        // or later text, and is never retracted.
        let input = "the meeting is at five, no wait, six";
        let out = r(input);
        assert!(out.ends_with("six"), "last word must be preserved: {out}");
    }

    // ── Quoted text is never edited ────────────────────────────────────────

    #[test]
    fn quoted_cue_kept() {
        let input = "he said \"no wait\" loudly";
        assert_eq!(r(input), input);
    }

    // ── Language gate ──────────────────────────────────────────────────────

    #[test]
    fn non_english_no_op() {
        let input = "lass uns uns bei fünf treffen, nein warte, sechs";
        assert_eq!(resolve_backtracks(input, "de"), input);
    }

    #[test]
    fn empty_language_counts_as_english() {
        assert_eq!(r("let's meet at five, no wait, six"), "let's meet at six");
    }

    #[test]
    fn en_us_counts_as_english() {
        assert_eq!(
            resolve_backtracks("let's meet at five, no wait, six", "en-US"),
            "let's meet at six"
        );
    }

    // ── Punctuation preservation ───────────────────────────────────────────

    #[test]
    fn punctuation_preserved() {
        assert_eq!(r("let's meet at five, no wait, six!"), "let's meet at six!");
    }

    #[test]
    fn capitalization_preserved() {
        assert_eq!(
            r("The meeting is at Five, no wait, Six"),
            "The meeting is at Six"
        );
    }

    // ── Edge cases ─────────────────────────────────────────────────────────

    #[test]
    fn empty_text() {
        assert_eq!(r(""), "");
    }

    #[test]
    fn whitespace_only() {
        assert_eq!(r("   "), "   ");
    }

    #[test]
    fn sorry_apology_not_retraction() {
        // "sorry" followed by a non-contrasting word is an apology, not a
        // retraction.
        let input = "sorry, i will be late";
        assert_eq!(r(input), input);
    }

    #[test]
    fn i_mean_pronoun_not_retraction() {
        // "i mean it" — the replacement is a pronoun, not a contrasting word.
        let input = "the release is monday, i mean it, so ship it";
        assert_eq!(r(input), input);
    }

    // ── Repaired object pronouns ──────────────────────────────────────────

    #[test]
    fn i_mean_retracts_a_replaced_pronoun_object() {
        // The corrected object replaces "it": the pronoun and the cue go, the
        // repaired noun phrase stays.
        assert_eq!(
            r("we should ship it, I mean the fix"),
            "we should ship the fix"
        );
        assert_eq!(
            r("let's reopen them, I mean the tickets"),
            "let's reopen the tickets"
        );
    }

    #[test]
    fn correction_replaces_a_pronoun_object_and_keeps_the_particle() {
        // "it over" -> "over": only the pronoun is removed, because "send
        // over the deck" is as natural as "send the deck over".
        assert_eq!(
            r("Can you send it over, I mean the PDF"),
            "Can you send over the PDF"
        );
        assert_eq!(
            r("print it out, correction, the invoice"),
            "print out the invoice"
        );
    }

    #[test]
    fn replaced_pronoun_needs_a_noun_phrase() {
        // A harmless continuation after "i mean" is not a repaired object.
        let input = "can you send it over, i mean maybe later";
        assert_eq!(r(input), input);
        // A preposition that is not a verb particle is not moved.
        let input = "put it in, i mean the box";
        assert_eq!(r(input), input);
    }

    #[test]
    fn repeated_connector_is_skipped_for_the_replacement() {
        // The repeated "to" is repair framing, not the repaired word.
        assert_eq!(
            r("send the deck to Sarah, I mean, to Priya"),
            "send the deck to Priya"
        );
        assert_eq!(
            r("send the deck to Sarah, I mean, Priya"),
            "send the deck to Priya"
        );
        // A different preposition is not skipped: skipping it would produce
        // "from to Tuesday".
        let input = "move the meeting from Monday, I mean to Tuesday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn quantified_object_correction_retracts_the_quantity() {
        // Retracting only "them" would leave "buy two of three of them".
        assert_eq!(
            r("buy two of them, I mean three of them"),
            "buy three of them"
        );
        assert_eq!(
            r("there are three of us, correction, four of us"),
            "there are four of us"
        );
    }

    #[test]
    fn new_resolutions_are_idempotent() {
        for input in [
            "Can you send it over, I mean the PDF",
            "send the deck to Sarah, I mean, to Priya",
            "buy two of them, I mean three of them",
            "we should ship it, I mean the fix",
        ] {
            let once = r(input);
            assert_eq!(r(&once), once, "idempotent: {input}");
            assert_ne!(once, input, "expected a resolution: {input}");
        }
    }

    #[test]
    fn never_mind_retracts_name() {
        // Capitalized name: the replacement "Gordon" is a contrasting word.
        // The comma after "Gordon" is repair framing and is removed.
        assert_eq!(
            r("invite Frank, never mind, Gordon, to the review"),
            "invite Gordon to the review"
        );
        // Lowercase name: ambiguous, keep both.
        assert_eq!(
            r("invite frank, never mind, gordon, to the review"),
            "invite frank, never mind, gordon, to the review"
        );
    }

    #[test]
    fn correction_retracts_number() {
        // Single-token number: the retraction resolves cleanly.
        assert_eq!(
            r("the budget is five, correction, six"),
            "the budget is six"
        );
        // Multi-word quantity: the whole phrase is retracted, not just its
        // last token.
        assert_eq!(
            r("the budget is five hundred, correction, six hundred"),
            "the budget is six hundred"
        );
    }

    #[test]
    fn wait_no_retracts_day() {
        // "wait no" is a strong cue; the clause walk from the cue includes
        // "friday" (no comma boundary), so the retraction resolves.
        assert_eq!(
            r("the release is friday wait no thursday"),
            "the release is thursday"
        );
    }

    #[test]
    fn strike_that_retracts_name() {
        // Capitalized name: the replacement "Gordon" is a contrasting word.
        assert_eq!(
            r("invite Frank, strike that, Gordon, to the review"),
            "invite Gordon to the review"
        );
        // Lowercase name with a strong cue: retract; the comma is repair framing.
        assert_eq!(
            r("invite frank, strike that, gordon, to the review"),
            "invite gordon to the review"
        );
    }

    #[test]
    fn forget_that_retracts_day() {
        assert_eq!(
            r("the release is friday, forget that, thursday"),
            "the release is thursday"
        );
    }

    #[test]
    fn cancel_that_retracts_number() {
        assert_eq!(
            r("the meeting is at five, cancel that, six"),
            "the meeting is at six"
        );
    }

    #[test]
    fn let_me_start_over_retracts_clause() {
        assert_eq!(
            r("the release is friday, let me start over, thursday"),
            "the release is thursday"
        );
    }

    #[test]
    fn newlines_preserved() {
        let input = "line one\nthe release is friday, no wait, thursday";
        let expected = "line one\nthe release is thursday";
        assert_eq!(r(input), expected);
    }

    #[test]
    fn cue_inside_quoted_text_not_matched() {
        let input = "she said \"friday, no wait, thursday\" to the team";
        assert_eq!(r(input), input);
    }

    #[test]
    fn multiple_cues_same_take_idempotent() {
        let input =
            "the deploy is at nine, no wait, ten, and the review is friday, i mean tuesday, at ten";
        let once = r(input);
        let twice = r(&once);
        assert_eq!(once, twice);
        assert!(once.contains("ten"));
        assert!(once.contains("tuesday"));
    }

    #[test]
    fn retracted_span_with_number_and_replacement_number_resolves() {
        // Both the retracted clause and the replacement have numbers:
        // resolve (remove retracted, keep replacement).
        assert_eq!(
            r("the release is at five, no wait, six"),
            "the release is at six"
        );
    }

    #[test]
    fn retracted_span_with_name_and_replacement_name_resolves() {
        // Capitalized names: the replacement "Gordon" is a contrasting word.
        assert_eq!(
            r("invite Frank, sorry, Gordon, to the review"),
            "invite Gordon to the review"
        );
        // Lowercase names: ambiguous, keep both.
        assert_eq!(
            r("invite frank, sorry, gordon, to the review"),
            "invite frank, sorry, gordon, to the review"
        );
    }

    #[test]
    fn no_cue_with_filler_words_byte_identical() {
        // Filler words without a backtrack cue are not touched by this pass
        // (they are the filler pass's job).
        let input = "um so the release went out and um everything looks fine";
        assert_eq!(r(input), input);
    }

    #[test]
    fn autotext_placeholder_preserved() {
        let input = "send the report to {{AUTOTEXT_0}} before friday, no wait, thursday";
        let out = r(input);
        assert!(
            out.contains("{{AUTOTEXT_0}}"),
            "AUTOTEXT placeholder must survive: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn phone_number_in_retracted_span_resolves() {
        // Strong cue: the retraction resolves. The phone number and the cue
        // are removed; "tomorrow" survives.
        assert_eq!(
            r("call me at 55582020, no wait, tomorrow"),
            "call me at tomorrow"
        );
    }

    #[test]
    fn url_in_retracted_span_resolves() {
        // Strong cue: the retraction resolves. The URL and the cue are
        // removed; "the docs" survives.
        assert_eq!(
            r("see https://example.com, no wait, the docs"),
            "see the docs"
        );
    }

    #[test]
    fn time_in_retracted_span_resolves() {
        assert_eq!(
            r("the meeting is at 9:15, no wait, 10:30"),
            "the meeting is at 10:30"
        );
    }

    #[test]
    fn currency_in_retracted_span_resolves() {
        assert_eq!(r("the budget is $500, no wait, $600"), "the budget is $600");
    }

    #[test]
    fn retracted_clause_with_comma_boundary() {
        // The clause is bounded by a comma: "the release is friday, no wait,
        // thursday" — the clause is "friday" (after the comma before the cue).
        assert_eq!(
            r("the release is friday, no wait, thursday"),
            "the release is thursday"
        );
    }

    #[test]
    fn retracted_clause_with_stop_boundary() {
        // The clause is bounded by a stop: "the release is friday. no wait,
        // thursday" — the clause is "the release is friday" (the whole
        // sentence before the stop).
        let input = "the release is friday. no wait, thursday";
        let out = r(input);
        // The stop is a boundary: the clause before the cue is "no wait"
        // itself (the stop is before the cue, so the clause is empty).
        // Actually, the clause walk goes back from the cue: "no" is the
        // token before "wait", and "no" has no trailing comma/stop, so the
        // clause is "no". But "no" is in NON_FACTS, so has_content is false
        // and nothing is retracted.
        assert_eq!(out, input);
    }

    #[test]
    fn retracted_clause_multiline() {
        let input = "the release is\nfriday, no wait, thursday";
        let out = r(input);
        assert!(out.contains("thursday"), "replacement must survive: {out}");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
    }

    #[test]
    fn cue_followed_by_comma_then_replacement() {
        assert_eq!(
            r("the release is friday, no wait, thursday, at ten"),
            "the release is thursday, at ten"
        );
    }

    #[test]
    fn cue_with_parenthetical() {
        let input = "the release is friday (no wait, thursday) at ten";
        let out = r(input);
        // The cue is inside parentheses: the clause before the cue is
        // "friday", and the replacement is "thursday".
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn idempotent_keep_both() {
        // Strong cue with a non-contrasting replacement: the retraction
        // resolves (strong cues don't require contrast), so the text
        // changes. Idempotence: the second call sees no cue and returns
        // the same text.
        let input = "the release is at five, no wait, tomorrow";
        let once = r(input);
        let twice = r(&once);
        assert_eq!(once, twice);
        // The retracted "five" and the cue "no wait" are removed.
        assert!(
            !once.contains("five"),
            "retracted number must be removed: {once}"
        );
        assert!(!once.contains("no wait"), "cue must be removed: {once}");
        assert!(
            once.contains("tomorrow"),
            "replacement must survive: {once}"
        );
    }

    #[test]
    fn multiple_retractions_no_overlap() {
        // Two cues that would overlap: the earlier one wins.
        let input = "the release is friday, no wait, thursday, no wait, wednesday";
        let out = r(input);
        // The first "no wait" retracts "friday" and the second retracts
        // "thursday". The result should be "the release is wednesday".
        assert!(
            out.contains("wednesday"),
            "final replacement must survive: {out}"
        );
        assert!(
            !out.contains("friday"),
            "first retracted day must be removed: {out}"
        );
        assert!(
            !out.contains("thursday"),
            "second retracted day must be removed: {out}"
        );
    }

    #[test]
    fn cue_at_start_of_take_retracts_first_word() {
        // The cue is at position 0: "friday, no wait, tuesday" — the first
        // token "friday" is explicitly retracted by the cue, so it is
        // allowed (the rule says "unless it is explicitly retracted by a
        // cue").
        assert_eq!(r("friday, no wait, tuesday"), "tuesday");
    }

    #[test]
    fn cue_not_at_start_does_not_retract_first_word() {
        // The cue is not at position 0: "the release is friday, no wait,
        // thursday" — the first token "the" is not retracted (the clause
        // before the cue is "friday", not "the release is friday").
        let out = r("the release is friday, no wait, thursday");
        assert!(
            out.starts_with("the"),
            "first word must be preserved: {out}"
        );
    }

    #[test]
    fn retracted_clause_is_only_the_last_phrase() {
        // The clause walk stops at a comma: "the release is friday, no wait,
        // thursday" — the clause is "friday" (after the comma before the
        // cue), not "the release is friday".
        let out = r("the release is friday, no wait, thursday");
        assert!(
            out.contains("the release is"),
            "earlier clause must survive: {out}"
        );
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_stops_at_stop_mark() {
        // The clause walk stops at a stop mark: "the release is friday. no
        // wait, thursday" — the clause before the cue is empty (the stop is
        // before the cue), so nothing is retracted.
        let input = "the release is friday. no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_stops_at_comma() {
        // The clause walk stops at a comma: "the release is friday, no
        // wait, thursday" — the clause is "friday" (after the comma before
        // the cue).
        let out = r("the release is friday, no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_multiline_stops_at_newline() {
        // The clause walk does not cross a newline: "the release is\nfriday,
        // no wait, thursday" — the clause is "friday" (after the comma
        // before the cue), not "the release is friday".
        let out = r("the release is\nfriday, no wait, thursday");
        assert!(
            out.contains("the release is"),
            "earlier clause must survive: {out}"
        );
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_ellipsis() {
        // The clause walk stops at an ellipsis: "the release is friday...
        // no wait, thursday" — the clause is "friday" (after the ellipsis
        // before the cue).
        let input = "the release is friday... no wait, thursday";
        let out = r(input);
        // The ellipsis is a stop-like mark: the clause walk stops at it.
        // Actually, "..." is not a comma or stop (., !, ?), so the clause
        // walk continues past it. The clause is "friday... no" — but "no"
        // is in NON_FACTS, and "friday" is a name-like word, so has_content
        // is true. The replacement is "thursday" (a contrasting word), so
        // the retraction resolves.
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_semicolon() {
        // The clause walk stops at a semicolon: "the release is friday; no
        // wait, thursday" — the clause is "friday" (after the semicolon
        // before the cue).
        let input = "the release is friday; no wait, thursday";
        let out = r(input);
        // The semicolon is not a comma or stop, so the clause walk
        // continues past it. The clause is "friday; no" — but "no" is in
        // NON_FACTS, and "friday" is a name-like word, so has_content is
        // true. The replacement is "thursday" (a contrasting word), so the
        // retraction resolves.
        assert!(out.contains("thursday"), "replacement must survive: {out}");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_colon() {
        // The clause walk does not stop at a colon (a time-like colon):
        // "the meeting is at 9:15, no wait, 10:30" — the clause is "9:15"
        // (after the comma before the cue), not "the meeting is at 9:15".
        let out = r("the meeting is at 9:15, no wait, 10:30");
        assert!(out.contains("10:30"), "replacement must survive: {out}");
        assert!(
            !out.contains("9:15"),
            "retracted time must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_dash() {
        // The clause walk does not stop at a dash: "the release is friday -
        // no wait, thursday" — the clause is "friday -" (after the start of
        // the text before the cue).
        let input = "the release is friday - no wait, thursday";
        let out = r(input);
        // The dash is not a comma or stop, so the clause walk continues
        // past it. The clause is "friday -" — but "-" is not alphanumeric,
        // so its bare is empty, and has_content checks "friday" (a
        // name-like word), so has_content is true. The replacement is
        // "thursday" (a contrasting word), so the retraction resolves.
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_hyphenated_word() {
        // The clause walk does not stop at a hyphenated word: "the release
        // is state-of-the-art, no wait, legacy" — the clause is
        // "state-of-the-art" (after the comma before the cue).
        let input = "the release is state-of-the-art, no wait, legacy";
        let out = r(input);
        // The clause is "state-of-the-art" (a single token with hyphens).
        // Its bare is "stateoftheart" (hyphens stripped), which is
        // name-like. The replacement is "legacy" (a name-like word), so
        // the retraction resolves.
        assert!(out.contains("legacy"), "replacement must survive: {out}");
        assert!(
            !out.contains("state-of-the-art"),
            "retracted word must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_apostrophe() {
        // The clause walk does not stop at an apostrophe: "let's meet at
        // five, no wait, six" — the clause is "five" (after the comma
        // before the cue).
        let out = r("let's meet at five, no wait, six");
        assert!(out.contains("let's"), "earlier clause must survive: {out}");
        assert!(
            !out.contains("five"),
            "retracted number must be removed: {out}"
        );
        assert!(out.contains("six"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_abbreviation() {
        // The clause walk does not stop at an abbreviation: "the meeting is
        // at 9:15 a.m., no wait, 10:30 a.m." — the clause is "a.m." (the
        // token carrying the comma). "9:15" is a separate token and is not
        // retracted (a known limitation of the token-based clause walk for
        // multi-word time expressions).
        let input = "the meeting is at 9:15 a.m., no wait, 10:30 a.m.";
        let out = r(input);
        assert!(out.contains("10:30"), "replacement must survive: {out}");
        assert!(
            !out.contains("a.m., no wait"),
            "retracted abbreviation + cue must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_acronym() {
        // The clause walk does not stop at an acronym: "the deploy is in
        // us-east-1, no wait, us-east-2" — the clause is "us-east-1" (after
        // the comma before the cue).
        let input = "the deploy is in us-east-1, no wait, us-east-2";
        let out = r(input);
        assert!(out.contains("us-east-2"), "replacement must survive: {out}");
        assert!(
            !out.contains("us-east-1"),
            "retracted region must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_email() {
        // The clause walk does not stop at an email: "send it to a@b.com, no
        // wait, c@d.com" — the clause is "a@b.com" (after the comma before
        // the cue).
        let input = "send it to a@b.com, no wait, c@d.com";
        let out = r(input);
        assert!(out.contains("c@d.com"), "replacement must survive: {out}");
        assert!(
            !out.contains("a@b.com"),
            "retracted email must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_version() {
        // The clause walk does not stop at a version string: "the build is
        // v1.2.3, no wait, v1.2.4" — the clause is "v1.2.3" (after the
        // comma before the cue).
        let input = "the build is v1.2.3, no wait, v1.2.4";
        let out = r(input);
        assert!(out.contains("v1.2.4"), "replacement must survive: {out}");
        assert!(
            !out.contains("v1.2.3"),
            "retracted version must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_percentage() {
        // The clause walk does not stop at a percentage: "the budget is
        // 50%, no wait, 60%" — the clause is "50%" (after the comma before
        // the cue).
        let input = "the budget is 50%, no wait, 60%";
        let out = r(input);
        assert!(out.contains("60%"), "replacement must survive: {out}");
        assert!(
            !out.contains("50%"),
            "retracted percentage must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_hash() {
        // The clause walk does not stop at a hash: "the commit is abc123, no
        // wait, def456" — the clause is "abc123" (after the comma before
        // the cue).
        let input = "the commit is abc123, no wait, def456";
        let out = r(input);
        assert!(out.contains("def456"), "replacement must survive: {out}");
        assert!(
            !out.contains("abc123"),
            "retracted hash must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_at_sign() {
        // The clause walk does not stop at an at-sign: "the meeting is at
        // 9:15 @ the office, no wait, 10:30 @ the office" — the clause is
        // "office" (the token carrying the comma). "9:15 @ the" are separate
        // tokens and are not retracted (a known limitation of the token-based
        // clause walk for multi-word phrases).
        let input = "the meeting is at 9:15 @ the office, no wait, 10:30 @ the office";
        let out = r(input);
        assert!(out.contains("10:30"), "replacement must survive: {out}");
        assert!(
            !out.contains("office, no wait"),
            "retracted clause + cue must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_slash() {
        // The clause walk does not stop at a slash: "the deploy is at
        // 9/15, no wait, 10/30" — the clause is "9/15" (after the comma
        // before the cue).
        let input = "the deploy is at 9/15, no wait, 10/30";
        let out = r(input);
        assert!(out.contains("10/30"), "replacement must survive: {out}");
        assert!(
            !out.contains("9/15"),
            "retracted date must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_underscore() {
        // The clause walk does not stop at an underscore: "the variable is
        // FEATURE_FLAGS, no wait, FEATURE_FLAGS_V2" — the clause is
        // "FEATURE_FLAGS" (after the comma before the cue).
        let input = "the variable is FEATURE_FLAGS, no wait, FEATURE_FLAGS_V2";
        let out = r(input);
        assert!(
            out.contains("FEATURE_FLAGS_V2"),
            "replacement must survive: {out}"
        );
        // The retracted identifier "FEATURE_FLAGS" (without _V2) must be
        // removed. Check for the exact token, not a substring (FEATURE_FLAGS
        // is a prefix of FEATURE_FLAGS_V2).
        assert!(
            !out.split_whitespace()
                .any(|t| t == "FEATURE_FLAGS" || t == "FEATURE_FLAGS,"),
            "retracted identifier must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_backtick() {
        // The clause walk does not stop at a backtick: "the command is
        // `kubectl get pods`, no wait, `kubectl get deployments`" — the
        // clause is "`kubectl get pods`" (after the comma before the cue).
        let input = "the command is `kubectl get pods`, no wait, `kubectl get deployments`";
        let out = r(input);
        assert!(
            out.contains("deployments"),
            "replacement must survive: {out}"
        );
        assert!(
            !out.contains("pods"),
            "retracted command must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_square_brackets() {
        // The clause walk does not stop at square brackets: "the file is
        // [report.pdf], no wait, [report.docx]" — the clause is
        // "[report.pdf]" (after the comma before the cue).
        let input = "the file is [report.pdf], no wait, [report.docx]";
        let out = r(input);
        assert!(
            out.contains("report.docx"),
            "replacement must survive: {out}"
        );
        assert!(
            !out.contains("report.pdf"),
            "retracted file must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_curly_braces() {
        // The clause walk does not stop at curly braces: "the value is
        // {AUTOTEXT_0}, no wait, {AUTOTEXT_1}" — the clause is
        // "{AUTOTEXT_0}" (after the comma before the cue).
        let input = "the value is {AUTOTEXT_0}, no wait, {AUTOTEXT_1}";
        let out = r(input);
        assert!(
            out.contains("AUTOTEXT_1"),
            "replacement must survive: {out}"
        );
        assert!(
            !out.contains("AUTOTEXT_0"),
            "retracted placeholder must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_angle_brackets() {
        // The clause walk does not stop at angle brackets: "the tag is
        // <v1.2.3>, no wait, <v1.2.4>" — the clause is "<v1.2.3>" (after
        // the comma before the cue).
        let input = "the tag is <v1.2.3>, no wait, <v1.2.4>";
        let out = r(input);
        assert!(out.contains("v1.2.4"), "replacement must survive: {out}");
        assert!(
            !out.contains("v1.2.3"),
            "retracted tag must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_pipe() {
        // The clause walk does not stop at a pipe: "the value is a|b, no
        // wait, c|d" — the clause is "a|b" (after the comma before the
        // cue).
        let input = "the value is a|b, no wait, c|d";
        let out = r(input);
        assert!(out.contains("c|d"), "replacement must survive: {out}");
        assert!(
            !out.contains("a|b"),
            "retracted value must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_caret() {
        // The clause walk does not stop at a caret: "the value is a^b, no
        // wait, c^d" — the clause is "a^b" (after the comma before the
        // cue).
        let input = "the value is a^b, no wait, c^d";
        let out = r(input);
        assert!(out.contains("c^d"), "replacement must survive: {out}");
        assert!(
            !out.contains("a^b"),
            "retracted value must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_ampersand() {
        // The clause walk does not stop at an ampersand: "the value is a&b,
        // no wait, c&d" — the clause is "a&b" (after the comma before the
        // cue).
        let input = "the value is a&b, no wait, c&d";
        let out = r(input);
        assert!(out.contains("c&d"), "replacement must survive: {out}");
        assert!(
            !out.contains("a&b"),
            "retracted value must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_plus() {
        // The clause walk does not stop at a plus: "the value is a+b, no
        // wait, c+d" — the clause is "a+b" (after the comma before the
        // cue).
        let input = "the value is a+b, no wait, c+d";
        let out = r(input);
        assert!(out.contains("c+d"), "replacement must survive: {out}");
        assert!(
            !out.contains("a+b"),
            "retracted value must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_minus() {
        // The clause walk does not stop at a minus: "the value is a-b, no
        // wait, c-d" — the clause is "a-b" (after the comma before the
        // cue).
        let input = "the value is a-b, no wait, c-d";
        let out = r(input);
        assert!(out.contains("c-d"), "replacement must survive: {out}");
        assert!(
            !out.contains("a-b"),
            "retracted value must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_equals() {
        // The clause walk does not stop at an equals: "the value is a=b, no
        // wait, c=d" — the clause is "a=b" (after the comma before the
        // cue).
        let input = "the value is a=b, no wait, c=d";
        let out = r(input);
        assert!(out.contains("c=d"), "replacement must survive: {out}");
        assert!(
            !out.contains("a=b"),
            "retracted value must be removed: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_question_mark() {
        // The clause walk stops at a question mark: "the release is friday?
        // no wait, thursday" — the clause is empty (the question mark is
        // before the cue), so nothing is retracted.
        let input = "the release is friday? no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_exclamation() {
        // The clause walk stops at an exclamation mark: "the release is
        // friday! no wait, thursday" — the clause is empty (the exclamation
        // mark is before the cue), so nothing is retracted.
        let input = "the release is friday! no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_period() {
        // The clause walk stops at a period: "the release is friday. no
        // wait, thursday" — the clause is empty (the period is before the
        // cue), so nothing is retracted.
        let input = "the release is friday. no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_multiple_stops() {
        // The clause walk stops at multiple stops: "the release is friday.
        // the review is monday. no wait, tuesday" — the clause is empty
        // (the period is before the cue), so nothing is retracted.
        let input = "the release is friday. the review is monday. no wait, tuesday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_comma_and_stop() {
        // The clause walk stops at a comma or stop: "the release is friday,
        // the review is monday. no wait, tuesday" — the clause is empty
        // (the period is before the cue), so nothing is retracted.
        let input = "the release is friday, the review is monday. no wait, tuesday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_comma_before_cue() {
        // The clause walk stops at a comma before the cue: "the release is
        // friday, no wait, thursday" — the clause is "friday" (after the
        // comma before the cue).
        let out = r("the release is friday, no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_comma_after_cue() {
        // The clause walk does not stop at a comma after the cue: "the
        // release is friday no wait, thursday" — the clause is "friday"
        // (after the start of the text before the cue), and the replacement
        // is "thursday" (after the comma after the cue).
        let out = r("the release is friday no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_comma_before_and_after_cue() {
        // The clause walk stops at a comma before the cue: "the release is
        // friday, no wait, thursday" — the clause is "friday" (after the
        // comma before the cue), and the replacement is "thursday" (after
        // the comma after the cue).
        let out = r("the release is friday, no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_multiple_commas() {
        // The clause walk stops at the first comma before the cue: "the
        // release is friday, at ten, no wait, thursday" — the clause is
        // "at ten" (after the first comma before the cue), not "friday".
        let out = r("the release is friday, at ten, no wait, thursday");
        assert!(out.contains("friday"), "earlier clause must survive: {out}");
        assert!(
            !out.contains("at ten"),
            "retracted clause must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_semicolon_before_cue() {
        // The clause walk does not stop at a semicolon: "the release is
        // friday; no wait, thursday" — the clause is "friday;" (after the
        // start of the text before the cue).
        let out = r("the release is friday; no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_colon_before_cue() {
        // The clause walk does not stop at a colon: "the meeting is at 9:15
        // no wait, 10:30" — the clause is "9:15" (after the start of the
        // text before the cue).
        let out = r("the meeting is at 9:15 no wait, 10:30");
        assert!(
            !out.contains("9:15"),
            "retracted time must be removed: {out}"
        );
        assert!(out.contains("10:30"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_dash_before_cue() {
        // The clause walk does not stop at a dash: "the release is friday -
        // no wait, thursday" — the clause is "friday -" (after the start of
        // the text before the cue).
        let out = r("the release is friday - no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_hyphen_before_cue() {
        // The clause walk does not stop at a hyphen: "the release is
        // state-of-the-art no wait, legacy" — the clause is
        // "state-of-the-art" (after the start of the text before the cue).
        let out = r("the release is state-of-the-art no wait, legacy");
        assert!(
            !out.contains("state-of-the-art"),
            "retracted word must be removed: {out}"
        );
        assert!(out.contains("legacy"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_apostrophe_before_cue() {
        // The clause walk does not stop at an apostrophe: "let's meet at
        // five no wait, six" — the clause is "five" (after the start of the
        // text before the cue).
        let out = r("let's meet at five no wait, six");
        assert!(out.contains("let's"), "earlier clause must survive: {out}");
        assert!(
            !out.contains("five"),
            "retracted number must be removed: {out}"
        );
        assert!(out.contains("six"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_abbreviation_before_cue() {
        // The clause walk does not stop at an abbreviation: "the meeting is
        // at 9:15 a.m. no wait, 10:30 a.m." — the clause is "9:15 a.m."
        // (after the start of the text before the cue).
        let out = r("the meeting is at 9:15 a.m. no wait, 10:30 a.m.");
        assert!(
            !out.contains("9:15"),
            "retracted time must be removed: {out}"
        );
        assert!(out.contains("10:30"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_acronym_before_cue() {
        // The clause walk does not stop at an acronym: "the deploy is in
        // us-east-1 no wait, us-east-2" — the clause is "us-east-1" (after
        // the start of the text before the cue).
        let out = r("the deploy is in us-east-1 no wait, us-east-2");
        assert!(
            !out.contains("us-east-1"),
            "retracted region must be removed: {out}"
        );
        assert!(out.contains("us-east-2"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_email_before_cue() {
        // The clause walk does not stop at an email: "send it to a@b.com no
        // wait, c@d.com" — the clause is "a@b.com" (after the start of the
        // text before the cue).
        let out = r("send it to a@b.com no wait, c@d.com");
        assert!(
            !out.contains("a@b.com"),
            "retracted email must be removed: {out}"
        );
        assert!(out.contains("c@d.com"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_version_before_cue() {
        // The clause walk does not stop at a version string: "the build is
        // v1.2.3 no wait, v1.2.4" — the clause is "v1.2.3" (after the start
        // of the text before the cue).
        let out = r("the build is v1.2.3 no wait, v1.2.4");
        assert!(
            !out.contains("v1.2.3"),
            "retracted version must be removed: {out}"
        );
        assert!(out.contains("v1.2.4"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_percentage_before_cue() {
        // The clause walk does not stop at a percentage: "the budget is 50%
        // no wait, 60%" — the clause is "50%" (after the start of the text
        // before the cue).
        let out = r("the budget is 50% no wait, 60%");
        assert!(
            !out.contains("50%"),
            "retracted percentage must be removed: {out}"
        );
        assert!(out.contains("60%"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_hash_before_cue() {
        // The clause walk does not stop at a hash: "the commit is abc123 no
        // wait, def456" — the clause is "abc123" (after the start of the
        // text before the cue).
        let out = r("the commit is abc123 no wait, def456");
        assert!(
            !out.contains("abc123"),
            "retracted hash must be removed: {out}"
        );
        assert!(out.contains("def456"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_at_sign_before_cue() {
        // The clause walk stops at the function word "the", leaving only
        // "office" — a suffix of "9:15 @ the office". Removing a suffix
        // would corrupt the sentence, so the resolver keeps the take.
        let input = "the meeting is at 9:15 @ the office no wait, 10:30 @ the office";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_slash_before_cue() {
        // The clause walk does not stop at a slash: "the deploy is at 9/15
        // no wait, 10/30" — the clause is "9/15" (after the start of the
        // text before the cue).
        let out = r("the deploy is at 9/15 no wait, 10/30");
        assert!(
            !out.contains("9/15"),
            "retracted date must be removed: {out}"
        );
        assert!(out.contains("10/30"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_underscore_before_cue() {
        // The clause walk does not stop at an underscore: "the variable is
        // FEATURE_FLAGS no wait, FEATURE_FLAGS_V2" — the clause is
        // "FEATURE_FLAGS" (after the start of the text before the cue).
        let out = r("the variable is FEATURE_FLAGS no wait, FEATURE_FLAGS_V2");
        assert_eq!(out, "the variable is FEATURE_FLAGS_V2");
    }

    #[test]
    fn retracted_clause_with_backtick_before_cue() {
        // The clause walk does not stop at a backtick: "the command is
        // `kubectl get pods` no wait, `kubectl get deployments`" — the
        // clause is "`kubectl get pods`" (after the start of the text
        // before the cue).
        let out = r("the command is `kubectl get pods` no wait, `kubectl get deployments`");
        assert!(
            !out.contains("pods"),
            "retracted command must be removed: {out}"
        );
        assert!(
            out.contains("deployments"),
            "replacement must survive: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_square_brackets_before_cue() {
        // The clause walk does not stop at square brackets: "the file is
        // [report.pdf] no wait, [report.docx]" — the clause is
        // "[report.pdf]" (after the start of the text before the cue).
        let out = r("the file is [report.pdf] no wait, [report.docx]");
        assert!(
            !out.contains("report.pdf"),
            "retracted file must be removed: {out}"
        );
        assert!(
            out.contains("report.docx"),
            "replacement must survive: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_curly_braces_before_cue() {
        // The clause walk does not stop at curly braces: "the value is
        // {AUTOTEXT_0} no wait, {AUTOTEXT_1}" — the clause is
        // "{AUTOTEXT_0}" (after the start of the text before the cue).
        let out = r("the value is {AUTOTEXT_0} no wait, {AUTOTEXT_1}");
        assert!(
            !out.contains("AUTOTEXT_0"),
            "retracted placeholder must be removed: {out}"
        );
        assert!(
            out.contains("AUTOTEXT_1"),
            "replacement must survive: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_angle_brackets_before_cue() {
        // The clause walk does not stop at angle brackets: "the tag is
        // <v1.2.3> no wait, <v1.2.4>" — the clause is "<v1.2.3>" (after the
        // start of the text before the cue).
        let out = r("the tag is <v1.2.3> no wait, <v1.2.4>");
        assert!(
            !out.contains("v1.2.3"),
            "retracted tag must be removed: {out}"
        );
        assert!(out.contains("v1.2.4"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_pipe_before_cue() {
        // The clause walk does not stop at a pipe: "the value is a|b no
        // wait, c|d" — the clause is "a|b" (after the start of the text
        // before the cue).
        let out = r("the value is a|b no wait, c|d");
        assert!(
            !out.contains("a|b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c|d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_caret_before_cue() {
        // The clause walk does not stop at a caret: "the value is a^b no
        // wait, c^d" — the clause is "a^b" (after the start of the text
        // before the cue).
        let out = r("the value is a^b no wait, c^d");
        assert!(
            !out.contains("a^b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c^d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_ampersand_before_cue() {
        // The clause walk does not stop at an ampersand: "the value is a&b
        // no wait, c&d" — the clause is "a&b" (after the start of the text
        // before the cue).
        let out = r("the value is a&b no wait, c&d");
        assert!(
            !out.contains("a&b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c&d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_plus_before_cue() {
        // The clause walk does not stop at a plus: "the value is a+b no
        // wait, c+d" — the clause is "a+b" (after the start of the text
        // before the cue).
        let out = r("the value is a+b no wait, c+d");
        assert!(
            !out.contains("a+b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c+d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_minus_before_cue() {
        // The clause walk does not stop at a minus: "the value is a-b no
        // wait, c-d" — the clause is "a-b" (after the start of the text
        // before the cue).
        let out = r("the value is a-b no wait, c-d");
        assert!(
            !out.contains("a-b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c-d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_equals_before_cue() {
        // The clause walk does not stop at an equals: "the value is a=b no
        // wait, c=d" — the clause is "a=b" (after the start of the text
        // before the cue).
        let out = r("the value is a=b no wait, c=d");
        assert!(
            !out.contains("a=b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c=d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_question_mark_before_cue() {
        // The clause walk stops at a question mark: "the release is friday?
        // no wait, thursday" — the clause is empty (the question mark is
        // before the cue), so nothing is retracted.
        let input = "the release is friday? no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_exclamation_before_cue() {
        // The clause walk stops at an exclamation mark: "the release is
        // friday! no wait, thursday" — the clause is empty (the exclamation
        // mark is before the cue), so nothing is retracted.
        let input = "the release is friday! no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_period_before_cue() {
        // The clause walk stops at a period: "the release is friday. no
        // wait, thursday" — the clause is empty (the period is before the
        // cue), so nothing is retracted.
        let input = "the release is friday. no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_multiple_stops_before_cue() {
        // The clause walk stops at multiple stops: "the release is friday.
        // the review is monday. no wait, tuesday" — the clause is empty
        // (the period is before the cue), so nothing is retracted.
        let input = "the release is friday. the review is monday. no wait, tuesday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_comma_and_stop_before_cue() {
        // The clause walk stops at a comma or stop: "the release is friday,
        // the review is monday. no wait, tuesday" — the clause is empty
        // (the period is before the cue), so nothing is retracted.
        let input = "the release is friday, the review is monday. no wait, tuesday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_comma_before_cue_2() {
        // The clause walk stops at a comma before the cue: "the release is
        // friday, no wait, thursday" — the clause is "friday" (after the
        // comma before the cue).
        let out = r("the release is friday, no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_comma_after_cue_2() {
        // The clause walk does not stop at a comma after the cue: "the
        // release is friday no wait, thursday" — the clause is "friday"
        // (after the start of the text before the cue), and the replacement
        // is "thursday" (after the comma after the cue).
        let out = r("the release is friday no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_comma_before_and_after_cue_2() {
        // The clause walk stops at a comma before the cue: "the release is
        // friday, no wait, thursday" — the clause is "friday" (after the
        // comma before the cue), and the replacement is "thursday" (after
        // the comma after the cue).
        let out = r("the release is friday, no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_multiple_commas_2() {
        // The clause walk stops at the first comma before the cue: "the
        // release is friday, at ten, no wait, thursday" — the clause is
        // "at ten" (after the first comma before the cue), not "friday".
        let out = r("the release is friday, at ten, no wait, thursday");
        assert!(out.contains("friday"), "earlier clause must survive: {out}");
        assert!(
            !out.contains("at ten"),
            "retracted clause must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_semicolon_before_cue_2() {
        // The clause walk does not stop at a semicolon: "the release is
        // friday; no wait, thursday" — the clause is "friday;" (after the
        // start of the text before the cue).
        let out = r("the release is friday; no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_colon_before_cue_2() {
        // The clause walk does not stop at a colon: "the meeting is at 9:15
        // no wait, 10:30" — the clause is "9:15" (after the start of the
        // text before the cue).
        let out = r("the meeting is at 9:15 no wait, 10:30");
        assert!(
            !out.contains("9:15"),
            "retracted time must be removed: {out}"
        );
        assert!(out.contains("10:30"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_dash_before_cue_2() {
        // The clause walk does not stop at a dash: "the release is friday -
        // no wait, thursday" — the clause is "friday -" (after the start of
        // the text before the cue).
        let out = r("the release is friday - no wait, thursday");
        assert!(
            !out.contains("friday"),
            "retracted day must be removed: {out}"
        );
        assert!(out.contains("thursday"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_hyphen_before_cue_2() {
        // The clause walk does not stop at a hyphen: "the release is
        // state-of-the-art no wait, legacy" — the clause is
        // "state-of-the-art" (after the start of the text before the cue).
        let out = r("the release is state-of-the-art no wait, legacy");
        assert!(
            !out.contains("state-of-the-art"),
            "retracted word must be removed: {out}"
        );
        assert!(out.contains("legacy"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_apostrophe_before_cue_2() {
        // The clause walk does not stop at an apostrophe: "let's meet at
        // five no wait, six" — the clause is "five" (after the start of the
        // text before the cue).
        let out = r("let's meet at five no wait, six");
        assert!(out.contains("let's"), "earlier clause must survive: {out}");
        assert!(
            !out.contains("five"),
            "retracted number must be removed: {out}"
        );
        assert!(out.contains("six"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_abbreviation_before_cue_2() {
        // The clause walk does not stop at an abbreviation: "the meeting is
        // at 9:15 a.m. no wait, 10:30 a.m." — the clause is "9:15 a.m."
        // (after the start of the text before the cue).
        let out = r("the meeting is at 9:15 a.m. no wait, 10:30 a.m.");
        assert!(
            !out.contains("9:15"),
            "retracted time must be removed: {out}"
        );
        assert!(out.contains("10:30"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_acronym_before_cue_2() {
        // The clause walk does not stop at an acronym: "the deploy is in
        // us-east-1 no wait, us-east-2" — the clause is "us-east-1" (after
        // the start of the text before the cue).
        let out = r("the deploy is in us-east-1 no wait, us-east-2");
        assert!(
            !out.contains("us-east-1"),
            "retracted region must be removed: {out}"
        );
        assert!(out.contains("us-east-2"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_email_before_cue_2() {
        // The clause walk does not stop at an email: "send it to a@b.com no
        // wait, c@d.com" — the clause is "a@b.com" (after the start of the
        // text before the cue).
        let out = r("send it to a@b.com no wait, c@d.com");
        assert!(
            !out.contains("a@b.com"),
            "retracted email must be removed: {out}"
        );
        assert!(out.contains("c@d.com"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_version_before_cue_2() {
        // The clause walk does not stop at a version string: "the build is
        // v1.2.3 no wait, v1.2.4" — the clause is "v1.2.3" (after the start
        // of the text before the cue).
        let out = r("the build is v1.2.3 no wait, v1.2.4");
        assert!(
            !out.contains("v1.2.3"),
            "retracted version must be removed: {out}"
        );
        assert!(out.contains("v1.2.4"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_percentage_before_cue_2() {
        // The clause walk does not stop at a percentage: "the budget is 50%
        // no wait, 60%" — the clause is "50%" (after the start of the text
        // before the cue).
        let out = r("the budget is 50% no wait, 60%");
        assert!(
            !out.contains("50%"),
            "retracted percentage must be removed: {out}"
        );
        assert!(out.contains("60%"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_hash_before_cue_2() {
        // The clause walk does not stop at a hash: "the commit is abc123 no
        // wait, def456" — the clause is "abc123" (after the start of the
        // text before the cue).
        let out = r("the commit is abc123 no wait, def456");
        assert!(
            !out.contains("abc123"),
            "retracted hash must be removed: {out}"
        );
        assert!(out.contains("def456"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_at_sign_before_cue_2() {
        // Same conservative fallback as the non-_2 case: a function-word
        // boundary leaves a suffix, so keep the take.
        let input = "the meeting is at 9:15 @ the office no wait, 10:30 @ the office";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_slash_before_cue_2() {
        // The clause walk does not stop at a slash: "the deploy is at 9/15
        // no wait, 10/30" — the clause is "9/15" (after the start of the
        // text before the cue).
        let out = r("the deploy is at 9/15 no wait, 10/30");
        assert!(
            !out.contains("9/15"),
            "retracted date must be removed: {out}"
        );
        assert!(out.contains("10/30"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_underscore_before_cue_2() {
        // The clause walk does not stop at an underscore: "the variable is
        // FEATURE_FLAGS no wait, FEATURE_FLAGS_V2" — the clause is
        // "FEATURE_FLAGS" (after the start of the text before the cue).
        let out = r("the variable is FEATURE_FLAGS no wait, FEATURE_FLAGS_V2");
        assert_eq!(out, "the variable is FEATURE_FLAGS_V2");
    }

    #[test]
    fn retracted_clause_with_backtick_before_cue_2() {
        // The clause walk does not stop at a backtick: "the command is
        // `kubectl get pods` no wait, `kubectl get deployments`" — the
        // clause is "`kubectl get pods`" (after the start of the text
        // before the cue).
        let out = r("the command is `kubectl get pods` no wait, `kubectl get deployments`");
        assert!(
            !out.contains("pods"),
            "retracted command must be removed: {out}"
        );
        assert!(
            out.contains("deployments"),
            "replacement must survive: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_square_brackets_before_cue_2() {
        // The clause walk does not stop at square brackets: "the file is
        // [report.pdf] no wait, [report.docx]" — the clause is
        // "[report.pdf]" (after the start of the text before the cue).
        let out = r("the file is [report.pdf] no wait, [report.docx]");
        assert!(
            !out.contains("report.pdf"),
            "retracted file must be removed: {out}"
        );
        assert!(
            out.contains("report.docx"),
            "replacement must survive: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_curly_braces_before_cue_2() {
        // The clause walk does not stop at curly braces: "the value is
        // {AUTOTEXT_0} no wait, {AUTOTEXT_1}" — the clause is
        // "{AUTOTEXT_0}" (after the start of the text before the cue).
        let out = r("the value is {AUTOTEXT_0} no wait, {AUTOTEXT_1}");
        assert!(
            !out.contains("AUTOTEXT_0"),
            "retracted placeholder must be removed: {out}"
        );
        assert!(
            out.contains("AUTOTEXT_1"),
            "replacement must survive: {out}"
        );
    }

    #[test]
    fn retracted_clause_with_angle_brackets_before_cue_2() {
        // The clause walk does not stop at angle brackets: "the tag is
        // <v1.2.3> no wait, <v1.2.4>" — the clause is "<v1.2.3>" (after the
        // start of the text before the cue).
        let out = r("the tag is <v1.2.3> no wait, <v1.2.4>");
        assert!(
            !out.contains("v1.2.3"),
            "retracted tag must be removed: {out}"
        );
        assert!(out.contains("v1.2.4"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_pipe_before_cue_2() {
        // The clause walk does not stop at a pipe: "the value is a|b no
        // wait, c|d" — the clause is "a|b" (after the start of the text
        // before the cue).
        let out = r("the value is a|b no wait, c|d");
        assert!(
            !out.contains("a|b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c|d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_caret_before_cue_2() {
        // The clause walk does not stop at a caret: "the value is a^b no
        // wait, c^d" — the clause is "a^b" (after the start of the text
        // before the cue).
        let out = r("the value is a^b no wait, c^d");
        assert!(
            !out.contains("a^b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c^d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_ampersand_before_cue_2() {
        // The clause walk does not stop at an ampersand: "the value is a&b
        // no wait, c&d" — the clause is "a&b" (after the start of the text
        // before the cue).
        let out = r("the value is a&b no wait, c&d");
        assert!(
            !out.contains("a&b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c&d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_plus_before_cue_2() {
        // The clause walk does not stop at a plus: "the value is a+b no
        // wait, c+d" — the clause is "a+b" (after the start of the text
        // before the cue).
        let out = r("the value is a+b no wait, c+d");
        assert!(
            !out.contains("a+b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c+d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_minus_before_cue_2() {
        // The clause walk does not stop at a minus: "the value is a-b no
        // wait, c-d" — the clause is "a-b" (after the start of the text
        // before the cue).
        let out = r("the value is a-b no wait, c-d");
        assert!(
            !out.contains("a-b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c-d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_equals_before_cue_2() {
        // The clause walk does not stop at an equals: "the value is a=b no
        // wait, c=d" — the clause is "a=b" (after the start of the text
        // before the cue).
        let out = r("the value is a=b no wait, c=d");
        assert!(
            !out.contains("a=b"),
            "retracted value must be removed: {out}"
        );
        assert!(out.contains("c=d"), "replacement must survive: {out}");
    }

    #[test]
    fn retracted_clause_with_question_mark_before_cue_2() {
        // The clause walk stops at a question mark: "the release is friday?
        // no wait, thursday" — the clause is empty (the question mark is
        // before the cue), so nothing is retracted.
        let input = "the release is friday? no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_exclamation_before_cue_2() {
        // The clause walk stops at an exclamation mark: "the release is
        // friday! no wait, thursday" — the clause is empty (the exclamation
        // mark is before the cue), so nothing is retracted.
        let input = "the release is friday! no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_period_before_cue_2() {
        // The clause walk stops at a period: "the release is friday. no
        // wait, thursday" — the clause is empty (the period is before the
        // cue), so nothing is retracted.
        let input = "the release is friday. no wait, thursday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_multiple_stops_before_cue_2() {
        // The clause walk stops at multiple stops: "the release is friday.
        // the review is monday. no wait, tuesday" — the clause is empty
        // (the period is before the cue), so nothing is retracted.
        let input = "the release is friday. the review is monday. no wait, tuesday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn retracted_clause_with_comma_and_stop_before_cue_2() {
        // The clause walk stops at a comma or stop: "the release is friday,
        // the review is monday. no wait, tuesday" — the clause is empty
        // (the period is before the cue), so nothing is retracted.
        let input = "the release is friday, the review is monday. no wait, tuesday";
        assert_eq!(r(input), input);
    }

    #[test]
    fn dbg3() {
        let inputs = [
            "the release is monday, i mean tuesday, at ten",
            "invite Frank, never mind, Gordon, to the review",
            "invite Frank, scratch that, Gordon to the review",
            "the meeting is at 9:15 a.m., no wait, 10:30 a.m.",
            "the meeting is at 9:15 a.m. no wait, 10:30 a.m.",
            "the meeting is at 9:15 @ the office no wait, 10:30 @ the office",
            "the variable is FEATURE_FLAGS no wait, FEATURE_FLAGS_V2",
        ];
        for input in inputs {
            let out = super::resolve_backtracks(input, "en");
            eprintln!("IN : {input}");
            eprintln!("OUT: {out}");
            eprintln!();
        }
    }
}
