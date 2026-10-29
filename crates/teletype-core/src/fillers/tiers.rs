//! Tiered positional filler removal.
//!
//! A filler word is only removed when its **positional context** proves it is a
//! spoken hesitation, not a meaningful word. Every other word, including a
//! filler word in a non-filler position, is preserved.
//!
//! Tiers (mirrored from the reference implementation):
//!
//! * **Tier 1: Hesitation fillers** (`um`, `uh`, `er`): removed anywhere,
//!   except at the very start of the text (dictation start markers, e.g.
//!   `"er I will check"`), where they are kept so a real word is never eaten.
//! * **Tier 2: Discourse fillers** (`like`, `you know`, `I mean`): removed
//!   only when clause-adjacent: punctuation on the far side of the filler
//!   (the filler is followed by a stop/clause mark, or the filler itself
//!   carries a stop/clause mark), or when it ends the text (trailing
//!   "that is the plan like").
//! * **Tier 3: Parenthetical softeners** (`actually`, `basically`): removed
//!   only when bracketed by commas on both sides, and never sentence-initial.
//!
//! Removal is always gated by [`FillerStrength`]:
//! * `None` → pass through, unchanged.
//! * `Hesitations` → Tier 1 only.
//! * `Fillers` → Tier 1 + Tier 2.
//! * `Full` → Tier 1 + Tier 2 + Tier 3.
//!
//! Protection (applied before any tier is considered):
//! * **Quoted text**: a filler word inside an unmatched `"` run is never
//!   edited.
//! * **Dictionary terms / AutoText triggers**: a token that is a custom word
//!   or trigger is never removed, even if it is a Tier-1 filler.
//! * **Numbers, URLs, emails, code identifiers**: never removed.
//!
//! Whitespace is preserved byte-for-byte: only the bytes of a removed token
//! are dropped; the exact spaces and newlines around it survive. A stop mark
//! stranded on a removed filler is transferred to the previous kept token when
//! that token has no stop of its own, so the sentence keeps its end
//! punctuation.
//!
//! English-only: skipped for non-Latin-script languages (gate: `en` / `en-*` /
//! `en_*` / empty-legacy; otherwise no-op), the same rule the ITN and
//! polish-gate passes use.

/// How aggressively fillers are removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillerStrength {
    /// No filler removal.
    None,
    /// Hesitation interjections only (Tier 1); nothing that could be a word
    /// the user meant, so discourse markers and softeners are kept.
    Hesitations,
    /// Hesitation + discourse fillers.
    Fillers,
    /// Hesitation + discourse + parenthetical softeners.
    Full,
}

impl FillerStrength {
    /// `true` when any removal happens at this strength.
    pub fn enabled(self) -> bool {
        !matches!(self, FillerStrength::None)
    }

    /// `true` when Tier-2 discourse markers are removed at this strength.
    pub fn includes_discourse(self) -> bool {
        matches!(self, FillerStrength::Fillers | FillerStrength::Full)
    }

    /// `true` when Tier-3 parenthetical softeners are removed at this strength.
    pub fn includes_parenthetical(self) -> bool {
        matches!(self, FillerStrength::Full)
    }
}

/// The built-in tier membership. A word's tier is fixed by these sets; the
/// user's custom list is always treated as Tier 1 at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Hesitation,
    Discourse,
    Parenthetical,
}

const HESITATIONS: &[&str] = &[
    "um", "uh", "er", "ah", "eh", "umm", "uhh", "err", "ahh", "ehh", "hmm", "hmmm", "hm", "mm",
    "mmm", "mmmh", "urm", "ugh", "ahem",
];
const DISCOURSE: &[&str] = &[
    "like",
    "you know",
    "i mean",
    "i guess",
    "i suppose",
    "i think",
    "right",
    "okay",
    "oh",
    "well",
    "so",
    "anyway",
    "anyways",
    "honestly",
    "kinda",
    "sorta",
    "kind of",
    "sort of",
];

/// The closed set of pure openers removed sentence-initially when they carry
/// their own comma ("well, that is good to know" → "that is good to know").
/// `i mean`, `you know` and `i think` are deliberately absent: they carry
/// emphasis, and removing them changes the register of what was said.
const LEADING_COMMA_MARKERS: &[&str] = &[
    "well",
    "so",
    "right",
    "oh",
    "okay",
    "anyway",
    "anyways",
    "honestly",
    "kinda",
    "sorta",
    "i guess",
    "i suppose",
];
const PARENTHETICALS: &[&str] = &[
    "actually",
    "basically",
    "literally",
    "frankly",
    "to be honest",
];

/// Looks up the built-in tier for a bare lowercase filler term.
fn tier_of(bare: &str) -> Option<Tier> {
    if HESITATIONS.contains(&bare) {
        Some(Tier::Hesitation)
    } else if DISCOURSE.contains(&bare) {
        Some(Tier::Discourse)
    } else if PARENTHETICALS.contains(&bare) {
        Some(Tier::Parenthetical)
    } else {
        None
    }
}

/// The tier of `bare`, treating the user's custom list as Tier 1.
fn tier_of_or_user(bare: &str, user_set: &std::collections::HashSet<String>) -> Option<Tier> {
    if user_set.contains(bare) {
        Some(Tier::Hesitation)
    } else {
        tier_of(bare)
    }
}

/// English-only gate for the filler pass, matching the ITN and polish-gate
/// passes. Empty (legacy) language counts as English.
pub fn should_run(language: &str) -> bool {
    let lower = language.to_lowercase();
    lower.is_empty() || lower == "en" || lower.starts_with("en-") || lower.starts_with("en_")
}

/// One whitespace-delimited token with the exact whitespace that followed it.
///
/// Tokens carry their trailing whitespace explicitly so [`build_output`] can
/// reconstruct the input byte-for-byte without any offset arithmetic.
#[derive(Debug, Clone)]
struct Token {
    /// The token text as it appeared, including any glued punctuation.
    text: String,
    /// The exact whitespace run that followed the token in the input.
    gap: String,
    /// The lowercase alphanumeric content ("er," → "er", "I" → "i").
    bare: String,
    /// Byte start offset in the original text (kept in sync with `gap`;
    /// asserted monotonic in [`build_output`]).
    start: usize,
    /// `true` when the token sits inside a quoted (unmatched `"`) run.
    quoted: bool,
}

impl Token {
    fn leading_punct(&self) -> char {
        self.text
            .chars()
            .find(|c| !c.is_alphanumeric())
            .unwrap_or('\0')
    }

    fn trailing_punct(&self) -> char {
        self.text
            .chars()
            .rev()
            .find(|c| !c.is_alphanumeric())
            .unwrap_or('\0')
    }

    fn starts_with_punct(&self) -> bool {
        self.text
            .chars()
            .next()
            .is_some_and(|c| !c.is_alphanumeric())
    }
}

fn is_stop_char(c: char) -> bool {
    matches!(c, '.' | '!' | '?')
}

/// The input to [`remove_filler_words_tiered`].
#[derive(Debug, Clone)]
pub struct FillerInput<'a> {
    pub text: &'a str,
    /// The user's custom filler list, lowercased. Treated as Tier 1.
    pub user_fillers: &'a [String],
    /// Dictionary terms, lowercased, that must never be removed.
    pub dictionary_terms: &'a [String],
    /// AutoText triggers, lowercased, that must never be removed.
    pub autotext_triggers: &'a [String],
}

/// Removes filler words from `input.text` according to `strength`.
///
/// Returns the text with removed tokens' bytes dropped and whitespace
/// preserved. If `strength` is [`FillerStrength::None`] or the language gate
/// is off, returns the text unchanged.
pub fn remove_filler_words_tiered(
    input: &FillerInput<'_>,
    language: &str,
    strength: FillerStrength,
) -> String {
    if !strength.enabled() || !should_run(language) {
        return input.text.to_string();
    }

    let user_set: std::collections::HashSet<String> = input
        .user_fillers
        .iter()
        .map(|w| w.to_lowercase())
        .collect();
    let dict_set: std::collections::HashSet<String> = input
        .dictionary_terms
        .iter()
        .map(|w| w.to_lowercase())
        .collect();
    let at_set: std::collections::HashSet<String> = input
        .autotext_triggers
        .iter()
        .map(|w| w.to_lowercase())
        .collect();

    let tokens = tokenise(input.text);
    let n = tokens.len();

    // Build the set of multi-word phrase candidates (2+ words) from the
    // built-in lists and the user's custom list. Longest-first so the
    // longest phrase wins at a given start position.
    let mut phrase_candidates: Vec<(String, Tier)> = Vec::new();
    for &w in HESITATIONS {
        if w.contains(' ') {
            phrase_candidates.push((w.to_string(), Tier::Hesitation));
        }
    }
    for &w in DISCOURSE {
        if w.contains(' ') {
            phrase_candidates.push((w.to_string(), Tier::Discourse));
        }
    }
    for &w in PARENTHETICALS {
        if w.contains(' ') {
            phrase_candidates.push((w.to_string(), Tier::Parenthetical));
        }
    }
    for w in input.user_fillers {
        let lower = w.to_lowercase();
        if lower.contains(' ') {
            phrase_candidates.push((lower, Tier::Hesitation));
        }
    }
    // Sort by word count descending, then by string length descending, so
    // longer phrases are tried first.
    phrase_candidates.sort_by(|a, b| {
        b.0.split(' ')
            .count()
            .cmp(&a.0.split(' ').count())
            .then_with(|| b.0.len().cmp(&a.0.len()))
    });

    let mut remove = vec![false; n];
    let mut strip_prev_comma: Vec<bool> = vec![false; n];
    let mut i = 0usize;
    while i < n {
        let tok = &tokens[i];
        if tok.quoted || tok.starts_with_punct() || tok.bare.is_empty() {
            i += 1;
            continue;
        }
        if is_protected_nonword(&tok.text)
            || dict_set.contains(&tok.bare)
            || at_set.contains(&tok.bare)
        {
            i += 1;
            continue;
        }

        // Try longest-first phrase matching at position i. A match makes the
        // whole span one filler decision; the span's tier then applies the
        // same positional rule the tier imposes on a single word.
        let mut matched = false;
        for (phrase, tier) in &phrase_candidates {
            let words: Vec<&str> = phrase.split(' ').collect();
            let w = words.len();
            if w < 2 || i + w > n {
                continue;
            }
            // Check the span: no token may be quoted, punct-led, protected,
            // and the span must not cross a newline.
            let mut valid = true;
            for j in 0..w {
                let t = &tokens[i + j];
                if t.quoted || t.starts_with_punct() || t.bare.is_empty() {
                    valid = false;
                    break;
                }
                if is_protected_nonword(&t.text)
                    || dict_set.contains(&t.bare)
                    || at_set.contains(&t.bare)
                {
                    valid = false;
                    break;
                }
                // A gap containing \n means the phrase would cross a line.
                if t.gap.contains('\n') {
                    valid = false;
                    break;
                }
            }
            if !valid {
                continue;
            }
            // Join bare forms and compare.
            let joined: String = (0..w)
                .map(|j| tokens[i + j].bare.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if joined != *phrase {
                continue;
            }
            matched = true;
            let first = i;
            let last = i + w - 1;
            // Sentence-initial slot: the first kept token is at position 0.
            // For a multi-word span the comma may sit on the last token
            // ("I guess,"); for a single token, `first == last`.
            let slot_bare = if w > 1 { joined } else { tok.bare.clone() };
            let slot_is_leading_comma = first == 0
                && (tokens[0].trailing_punct() == ',' || tokens[last].trailing_punct() == ',');
            // A user-list phrase is unconditional Tier 1; a built-in phrase
            // applies its tier's positional rule.
            let is_user_phrase = user_set.contains(phrase);
            let (do_remove, strip) = if is_user_phrase {
                (true, false)
            } else {
                let ctx = SpanContext {
                    tokens: &tokens,
                    remove: &remove,
                    user_set: &user_set,
                    strength,
                    leading: (slot_is_leading_comma, &slot_bare),
                };
                decide_span(*tier, first, last, &ctx)
            };
            if do_remove {
                remove[first..=last].fill(true);
                if strip {
                    if first > 0 {
                        strip_prev_comma[first - 1] = true;
                    }
                    // A comma-bracketed multi-word span carries the closing
                    // comma on its last token; strip it there as well.
                    if tokens[last].trailing_punct() == ',' {
                        strip_prev_comma[last] = true;
                    }
                }
            }
            i = last + 1;
            break;
        }
        if matched {
            continue;
        }

        // Fall back to the single-token lookup. A user-list entry is
        // unconditional Tier 1 (it overrides the start-of-text and
        // end-of-text guards); a built-in entry applies its tier's rule.
        let is_user = user_set.contains(&tok.bare);
        let tier = match tier_of_or_user(&tok.bare, &user_set) {
            Some(t) => t,
            None => {
                i += 1;
                continue;
            }
        };
        let slot_is_leading_comma =
            i == 0 && (tokens[0].trailing_punct() == ',' || tokens[i].trailing_punct() == ',');
        let ctx = SpanContext {
            tokens: &tokens,
            remove: &remove,
            user_set: &user_set,
            strength,
            leading: (slot_is_leading_comma, &tok.bare),
        };
        let (do_remove, strip) = decide_span(tier, i, i, &ctx);
        if do_remove || is_user {
            remove[i] = true;
            if strip && i > 0 {
                strip_prev_comma[i - 1] = true;
            }
        }
        i += 1;
    }

    build_output(input.text, &tokens, &remove, &strip_prev_comma)
}

/// Context passed to [`decide_span`] so the positional rule can inspect the
/// surrounding tokens without a long argument list.
struct SpanContext<'a> {
    tokens: &'a [Token],
    remove: &'a [bool],
    user_set: &'a std::collections::HashSet<String>,
    strength: FillerStrength,
    /// `(slot_is_leading_comma, slot_bare)`: whether this span sits in the
    /// sentence-initial comma slot ("well, ..." / "I guess, ...") and the bare
    /// form of the span's first kept token. `slot_bare` is the span's joined
    /// phrase for a multi-word span, the token's bare form for a single one.
    leading: (bool, &'a str),
}

/// Decides whether the filler span `first..=last` is removed, applying the
/// positional rule the span's tier imposes on a single word. The span is
/// guaranteed to be unquoted, non-punct-led, and non-protected by the caller.
/// Returns `(remove, strip_prev_comma)`: whether to remove the span and
/// whether to strip the trailing comma from the previous kept token.
fn decide_span(tier: Tier, first: usize, last: usize, ctx: &SpanContext) -> (bool, bool) {
    let n = ctx.tokens.len();
    let tokens = ctx.tokens;
    let remove = ctx.remove;
    let user_set = ctx.user_set;
    let strength = ctx.strength;
    // The next kept token after the span: used for the clause rule and the
    // sentence-initial guard.
    let next_kept = (last + 1..n).find(|&j| !remove[j]);
    // A leading discourse marker followed by its own comma is
    // removed (with the comma) only from the closed pure-opener set. `i mean`,
    // `you know` and `i think` are kept sentence-initially because they carry
    // emphasis. The slot is further protected when it directly follows a kept
    // emphasis marker ("I think, you know, it is ready", no cascade past the kept
    // leading marker).
    let (slot_is_leading_comma, slot_bare) = ctx.leading;
    let leading_comma_marker = slot_is_leading_comma
        && LEADING_COMMA_MARKERS.contains(&slot_bare)
        && kept_emphasis_marker_at_start(tokens, remove).is_none();
    let prev_has_comma = first > 0 && tokens[first - 1].trailing_punct() == ',';
    // A filler is line-final when it sits at the end of a line: its gap
    // carries the newline ("Greeting line um\n"), or it is the last token in
    // a multi-line text ("...Sign-off line like" where the text has \n
    // elsewhere). A line-final hesitation is still a hesitation and is
    // removed; the newline survives in build_output. A text-final filler in a
    // single-line text ("the build passes um") is kept.
    let is_multi_line = tokens.iter().any(|t| t.gap.contains('\n'));
    let line_final = tokens[last].gap.contains('\n') || (last == n - 1 && is_multi_line);
    match tier {
        Tier::Hesitation => {
            // Removed anywhere, except: the very first token of a single-line
            // text (dictation start marker, "er I will check") and a
            // hesitation trailing a single-line text ("the build passes um"),
            // where there is no following clause mark to prove it is a
            // hesitation. In a multi-line text, a leading hesitation is a
            // line-initial hesitation ("um start\n") and is removed; a
            // line-final hesitation ("Greeting line um\n") is also removed.
            let remove = if first == 0 {
                // Leading token: removed only in a multi-line text (line-initial
                // hesitation), kept in a single-line text (dictation start
                // marker).
                is_multi_line
            } else {
                next_kept.is_some() || line_final
            };
            // Hesitations keep the previous comma ("okay, um, let us start"
            // → "okay, let us start").
            (remove, false)
        }
        Tier::Discourse => {
            if !strength.includes_discourse() {
                return (false, false);
            }
            let own_comma = tokens[last].trailing_punct() == ',';
            let prev_comma = prev_has_comma;
            let next_comma = next_kept.is_some_and(|j| tokens[j].leading_punct() == ',');
            // A line-final discourse marker in a multi-line text is a
            // hesitation about ending the line ("Sign-off line like"): remove
            // it. Single-line trailing fillers ("that is the plan like") are
            // kept.
            let line_final_discourse = line_final && next_kept.is_none();
            if own_comma || (prev_comma && next_comma) || line_final_discourse {
                // A leading marker that carries its own comma and is a pure
                // opener ("well, that is good to know") is a hesitation about
                // starting and is removed with its comma. A kept emphasis marker
                // ("i mean", "you know", "i think") stays, and so does the
                // marker in the comma slot directly after it ("I think, you
                // know, it is ready").
                let remove = if leading_comma_marker {
                    true
                } else if first == 0 {
                    // Sentence-initial marker without its own comma ("I mean,
                    // the tests pass" / "you know, um, the tests pass"): the
                    // leading emphasis markers ("i mean", "you know", "i think") are kept,
                    // and so is the comma slot right after one.
                    false
                } else {
                    // Clause-internal: a comma-bracketed or comma-carrying
                    // marker is removed unless the word after it starts a new
                    // clause ("I mean, we should ship" / "I guess, we are
                    // ready"). A comma-bracketed marker whose clause after the
                    // second comma begins with a verb is removed ("the tests,
                    // you know, are passing now" → "the tests are passing
                    // now") even though "are" is a form of "be": the "are"
                    // belongs to the clause, it does not start one.
                    let clause_starts_verb = prev_comma
                        && own_comma
                        && next_kept.is_some_and(|j| is_verb_form(&tokens[j].bare));
                    next_kept.is_none_or(|j| {
                        !is_sentence_initial(j, first, tokens, remove) || clause_starts_verb
                    })
                };
                // Strip the previous comma when the marker carries its own
                // comma and the previous token also has one ("I, like, said
                // this" → "I said this").
                let strip = remove && prev_comma && (own_comma || next_comma);
                (remove, strip)
            } else {
                (false, false)
            }
        }
        Tier::Parenthetical => {
            if !strength.includes_parenthetical() {
                return (false, false);
            }
            // Removed only when bracketed by commas on both sides, and never
            // sentence-initial.
            let prev_comma = prev_has_comma;
            let next_comma = tokens[last].trailing_punct() == ','
                || next_kept.is_some_and(|j| tokens[j].leading_punct() == ',');
            let sentence_initial = (0..first).all(|j| {
                remove[j]
                    || tokens[j].bare.is_empty()
                    || tier_of_or_user(&tokens[j].bare, user_set).is_some()
            });
            let remove = prev_comma && next_comma && !sentence_initial;
            // Parentheticals are bracketed by commas, so strip the previous
            // comma ("it is, basically, a test" → "it is a test"). For a
            // multi-word span the trailing comma sits on the span's last
            // token, so mark that one too ("it is, kind of, a mess" → "it is
            // a mess").
            (remove, remove && prev_comma)
        }
    }
}

/// The kept emphasis marker at the very start of the text: a leading `i mean`,
/// `you know` or `i think`. Returns its bare form when one is present.
fn kept_emphasis_marker_at_start(tokens: &[Token], remove: &[bool]) -> Option<&'static str> {
    const EMPHASIS_MARKERS: &[&str] = &["i mean", "you know", "i think"];
    let mut j = 0usize;
    while j < tokens.len() && remove[j] {
        j += 1;
    }
    if j >= tokens.len() {
        return None;
    }
    for m in EMPHASIS_MARKERS {
        let words: Vec<&str> = m.split(' ').collect();
        if j + words.len() <= tokens.len()
            && (0..words.len()).all(|k| !remove[j + k] && tokens[j + k].bare == words[k])
        {
            return Some(m);
        }
    }
    None
}

/// The verb forms that can open the clause after a comma-bracketed marker
/// ("the tests, you know, are passing now" / "it is, honestly, not that bad").
/// A form of "be" here belongs to the clause after the marker; it does not
/// start a new clause, so the marker is removed.
const CLAUSE_VERBS: &[&str] = &["is", "are", "was", "were", "be", "being", "been"];

/// True when `bare` is a form of "be" that can open a clause after a
/// comma-bracketed marker.
fn is_verb_form(bare: &str) -> bool {
    CLAUSE_VERBS.contains(&bare)
}

/// True when the token at index `j` starts a new clause or sentence: it is
/// capitalised and the previous kept token ends with a stop mark (`.`, `!`,
/// `?`) or is absent (start of text), OR it is a subject pronoun (I, you,
/// he, she, it, we, they) or a form of "be" (is, are, was, were, be, being,
/// been) or the demonstrative "that". This is used to keep discourse markers
/// that introduce a new clause ("I mean, we should ship" / "I guess, we are
/// ready") rather than remove them.
fn is_sentence_initial(j: usize, span_start: usize, tokens: &[Token], remove: &[bool]) -> bool {
    let bare = &tokens[j].bare;
    // Subject pronouns and forms of "be" mark a new clause even when
    // lowercase after a comma.
    const SUBJECT_LIKE: &[&str] = &[
        "i", "you", "he", "she", "it", "we", "they", "is", "are", "was", "were", "be", "being",
        "been", "that",
    ];
    if SUBJECT_LIKE.contains(&bare.as_str()) {
        return true;
    }
    // Case must come from the original token text: `bare` is lowercased, so
    // reading it here made the capitalisation branch unreachable.
    let Some(c) = tokens[j].text.chars().find(|c| c.is_alphabetic()) else {
        return false;
    };
    if !c.is_uppercase() {
        return false;
    }
    // The previous kept token BEFORE the span: the span itself is not marked
    // in `remove` yet, so searching back from `j` would find the filler and
    // never the sentence punctuation it follows.
    let prev_kept = (0..span_start).rev().find(|&k| !remove[k]);
    match prev_kept {
        None => true, // Start of text.
        Some(k) => is_stop_char(tokens[k].trailing_punct()),
    }
}

/// Builds the final text by dropping removed tokens (and transferring a
/// stranded trailing stop to the previous kept token) while preserving every
/// other byte, including whitespace, exactly. Gaps are carried explicitly
/// on each token, so the reconstruction needs no offset arithmetic and is
/// byte-exact when nothing is removed (see `identity_round_trip`).
fn build_output(
    text: &str,
    tokens: &[Token],
    remove: &[bool],
    strip_prev_comma: &[bool],
) -> String {
    if tokens.is_empty() {
        // No tokens means the input is empty or pure whitespace: nothing to
        // remove, return it untouched (byte-exact, no trim).
        return text.to_string();
    }

    // For each removed token carrying a stop ("um.", the sentence end
    // marker stuck to the filler), mark the previous kept token to gain that
    // stop (only if that token has no stop of its own), so the sentence keeps
    // its end punctuation.
    let mut append_stop: Vec<Option<char>> = vec![None; tokens.len()];
    for (i, tok) in tokens.iter().enumerate() {
        if remove[i] {
            // The previous *kept* token (skipping removed fillers), so a run
            // like "um so I" transfers the stranded stop to the right place.
            let prev_kept = (0..i).rev().find(|&k| !remove[k]);
            if let Some(p) = prev_kept {
                if !is_stop_char(tokens[p].trailing_punct()) {
                    if is_stop_char(tok.leading_punct()) {
                        append_stop[p] = Some(tok.leading_punct());
                    } else if is_stop_char(tok.trailing_punct()) {
                        // The filler carries the sentence's stop
                        // ("the build passes um." / "Greeting um.\nSecond
                        // line"). Without this the stop vanished with the
                        // filler and the sentence lost its end punctuation.
                        append_stop[p] = Some(tok.trailing_punct());
                    }
                }
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    // Mark kept tokens whose trailing gap whitespace should be trimmed because
    // the next token is removed and sits on a new line ("Greeting line um\n"
    // → "Greeting line\n") or is the last token on a line ("Sign-off line
    // like" → "Sign-off line").
    let is_multi_line = tokens.iter().any(|t| t.gap.contains('\n'));
    let mut trim_gap: Vec<bool> = vec![false; tokens.len()];
    for (i, tok) in tokens.iter().enumerate() {
        if remove[i] && i > 0 && !remove[i - 1] {
            let line_final = tok.gap.contains('\n') || (i == tokens.len() - 1 && is_multi_line);
            if line_final {
                trim_gap[i - 1] = true;
            }
        }
    }
    for (i, tok) in tokens.iter().enumerate() {
        debug_assert!(
            tok.start >= cursor,
            "token offsets must be monotonic (token {i} at {}, cursor {})",
            tok.start,
            cursor
        );
        if tok.start > cursor {
            let prefix = &text[cursor..tok.start];
            // Extra whitespace before a removed line-final filler is part of
            // the whitespace run that `trim_gap` already shortens for the
            // previous token; keep it out of the output so a line does not
            // end with stranded spaces.
            if remove[i] && i > 0 && trim_gap[i - 1] {
                out.push_str(prefix.trim_end());
            } else {
                out.push_str(prefix);
            }
        }
        if remove[i] {
            // Preserve line structure: if the removed token's gap contains a
            // newline, emit the newline (with any trailing whitespace after it)
            // so lines don't merge. For example, "line one um\nline two" →
            // "line one\nline two" (the \n from um's gap is kept).
            if tok.gap.contains('\n') {
                if let Some(nl_pos) = tok.gap.find('\n') {
                    out.push_str(&tok.gap[nl_pos..]);
                }
            }
        } else {
            let text_to_emit = if strip_prev_comma[i] && tok.trailing_punct() == ',' {
                &tok.text[..tok.text.len() - 1]
            } else {
                &tok.text
            };
            out.push_str(text_to_emit);
            if let Some(stop) = append_stop[i] {
                out.push(stop);
            }
            // Emit the gap, trimming trailing whitespace if the next token is
            // removed and on a new line.
            let gap_to_emit = if trim_gap[i] {
                tok.gap.trim_end().to_string()
            } else {
                tok.gap.clone()
            };
            out.push_str(&gap_to_emit);
        }
        cursor = tok.start + tok.text.len() + tok.gap.len();
    }
    if cursor < text.len() {
        out.push_str(&text[cursor..]);
    }
    out
}

/// Tokenises `text` into whitespace-delimited tokens, each carrying the exact
/// whitespace run (`gap`) that follows it, and flags, per token, whether it
/// sits inside a quoted (unmatched `"`) run.
///
/// Invariant: `text[..]` is exactly the concatenation of every token's
/// `text + gap` plus any leading whitespace before the first token, so
/// [`build_output`] is byte-exact when nothing is removed.
fn tokenise(text: &str) -> Vec<Token> {
    // Quote state per byte offset: quoted_at[b] = true when the byte at offset
    // b is inside a quoted run.
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
        // The piece ends at the LAST whitespace char of its run: everything
        // before it is the token, everything after is the exact gap.
        let split = piece
            .char_indices()
            .rfind(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(piece.len());
        let tok = &piece[..split];
        let gap = &piece[split..];
        // A piece that is pure whitespace (leading whitespace or blank lines)
        // carries no token; its bytes survive via the leading-slice and the
        // previous token's gap in build_output.
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

/// True when `token` is a number, URL, email, or code identifier: a protected
/// non-word span that must never be removed.
fn is_protected_nonword(token: &str) -> bool {
    let bare = token
        .trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '.' | ':' | '/' | '@' | '-'));
    if bare.is_empty() {
        return false;
    }
    is_number(bare) || is_url(bare) || is_email(bare) || is_code_identifier(bare)
}

fn is_number(w: &str) -> bool {
    let s = w.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
    if s.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    let mut seen_digit = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            seen_digit = true;
        } else if c == '.' {
            if seen_dot {
                return false;
            }
            seen_dot = true;
        } else if c == '-' {
            if s.len() > 1 && !s.starts_with('-') {
                return false;
            }
        } else {
            return false;
        }
    }
    seen_digit
}

fn is_url(w: &str) -> bool {
    let lower = w.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.")
}

fn is_email(w: &str) -> bool {
    if w.matches('@').count() != 1 {
        return false;
    }
    let Some((local, domain)) = w.split_once('@') else {
        return false;
    };
    !local.is_empty() && !domain.is_empty() && domain.contains('.')
}

fn is_code_identifier(w: &str) -> bool {
    if w.contains("::") || w.contains(".rs") || w.contains(".cpp") {
        return true;
    }
    if w.contains('_') {
        let segments: Vec<&str> = w.split('_').filter(|s| !s.is_empty()).collect();
        if segments.len() >= 2
            && segments
                .iter()
                .all(|s| s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
        {
            return true;
        }
    }
    // CamelCase/PascalCase only: a single leading capital is just an
    // auto-capitalised English word ("Well", "Okay", "Right"), not an
    // identifier, and protecting it would make sentence-initial filler
    // removal dead in real transcripts.
    if w.len() >= 4
        && w.chars().next().is_some_and(|c| c.is_uppercase())
        && w.chars().skip(1).any(|c| c.is_ascii_uppercase())
    {
        return true;
    }
    if w.len() >= 2 && w.chars().all(|c| c.is_ascii_uppercase()) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, strength: FillerStrength) -> String {
        remove_filler_words_tiered(
            &FillerInput {
                text,
                user_fillers: &[],
                dictionary_terms: &[],
                autotext_triggers: &[],
            },
            "en",
            strength,
        )
    }

    /// Every single-token entry in `HESITATIONS` is removed at the lightest
    /// strength. Iterates the real list, so adding a hesitation cannot go
    /// untested.
    #[test]
    fn hesitations_removes_every_tier1_hesitation() {
        let mut checked = 0;
        for word in HESITATIONS.iter().filter(|w| !w.contains(' ')) {
            let text = format!("the plan is {word} to ship on Friday");
            assert_eq!(
                run(&text, FillerStrength::Hesitations),
                "the plan is to ship on Friday",
                "hesitation {word:?} not removed at Hesitations strength"
            );
            checked += 1;
        }
        assert!(checked >= 10, "expected the hesitation list to be covered");
    }

    #[test]
    fn tier1_removed_anywhere() {        assert_eq!(
            run("hello um world", FillerStrength::Fillers),
            "hello world"
        );
        // A hesitation marker at the very start of the text is kept so a
        // real word is never eaten ("er I will check" → "I will check").
        assert_eq!(run("um hello", FillerStrength::Fillers), "um hello");
        assert_eq!(
            run("er I will check", FillerStrength::Fillers),
            "er I will check"
        );
    }

    #[test]
    fn tier2_requires_clause() {
        assert_eq!(run("I like pizza", FillerStrength::Fillers), "I like pizza");
        // When a parenthetical filler is removed from between
        // two commas, both commas that bracketed it go.
        assert_eq!(
            run("I, like, love this", FillerStrength::Fillers),
            "I love this"
        );
    }

    #[test]
    fn tier3_requires_commas_and_not_initial() {
        assert_eq!(
            run("It is, basically, the best", FillerStrength::Full),
            "It is the best"
        );
        assert_eq!(
            run("basically it is the best", FillerStrength::Full),
            "basically it is the best"
        );
    }

    #[test]
    fn none_passthrough() {
        assert_eq!(
            run("um I like pizza", FillerStrength::None),
            "um I like pizza"
        );
    }

    /// A sentence-initial discourse marker is a normal dictation word, not a
    /// code identifier. Auto-capitalised transcripts ("Well, ...") must be
    /// cleaned exactly like their lowercase form; the code-identifier
    /// protection used to consume every capitalised word of 4+ letters.
    #[test]
    fn capitalized_discourse_marker_is_removed() {
        assert_eq!(
            run("Well, that is good to know", FillerStrength::Fillers),
            "that is good to know"
        );
        assert_eq!(
            run("Okay, let us go", FillerStrength::Fillers),
            "let us go"
        );
    }

    /// A capitalised word that opens a new sentence after a stop mark must be
    /// recognised by `is_sentence_initial`, so a leading emphasis marker that
    /// introduces it is kept ("We shipped it. I mean, Sarah approved.").
    #[test]
    fn capitalized_sentence_start_keeps_emphasis_marker() {
        assert_eq!(
            run(
                "We shipped it. I mean, Sarah approved.",
                FillerStrength::Fillers
            ),
            "We shipped it. I mean, Sarah approved."
        );
    }

    /// Removing a line-final filler must not leave the extra whitespace that
    /// separated it from the previous word stranded before the newline.
    #[test]
    fn removing_line_filler_trims_extra_spaces() {
        assert_eq!(
            run("Greeting line  um\nnext line", FillerStrength::Fillers),
            "Greeting line\nnext line"
        );
    }

    /// A sentence stop stuck to a removed filler belongs to the sentence: it
    /// must move to the previous kept word, at a line end as well as at the
    /// very end of the text. The old guard skipped every filler that carried a
    /// stop, so the period was dropped with the filler.
    #[test]
    fn removed_filler_transfers_its_stop() {
        assert_eq!(
            run("Greeting um.\nSecond line", FillerStrength::Fillers),
            "Greeting.\nSecond line"
        );
        // Last token of a multi-line text: a line-final hesitation, so it is
        // removed and the stop moves to "Greeting".
        assert_eq!(
            run("line one\nGreeting um.", FillerStrength::Fillers),
            "line one\nGreeting."
        );
        // Single-line text-final hesitation stays put (see `decide_span`), so
        // its own stop stays with it.
        assert_eq!(
            run("The build passes um.", FillerStrength::Full),
            "The build passes um."
        );
    }

    /// Property: when the pass removes nothing, the output must equal the
    /// input byte-for-byte: double spaces, tabs, CRLF, leading/trailing
    /// whitespace, and blank lines all survive exactly.
    #[test]
    fn identity_round_trip() {
        let cases = [
            "no fillers here",
            "double  spaces  and   tabs\there",
            "crlf\r\nsecond\r\nthird",
            "  leading and trailing  ",
            "blank\n\n\nlines",
            "\t\nmixed\n  \t\r\n  ",
            "punct !?.,;: kept",
            "",    // empty input
            "   ", // whitespace-only input
        ];
        for text in cases {
            let out = remove_filler_words_tiered(
                &FillerInput {
                    text,
                    user_fillers: &[],
                    dictionary_terms: &[],
                    autotext_triggers: &[],
                },
                "en",
                FillerStrength::Full,
            );
            assert_eq!(out, text, "identity round-trip failed for {text:?}");
        }
    }
}





