//! Polish quality scoring for the text-transform stage.
//!
//! "Polish" is Teletype's post-ASR text transform (see
//! `teletype_core::transforms::engine`): the raw transcript is run through a
//! model that removes fillers, applies sentence casing and terminal
//! punctuation, and (for lists) renders bullets. The transform may also fall
//! back to the raw input on failure, so a polished string can be *identical*
//! to the reference — in which case the casing/punctuation checks fail, which
//! is the correct signal that the polish stage did not run.
//!
//! The rubric is deterministic: `score_polish(reference, polished)` returns a
//! per-check pass/fail list plus a 0-100 weighted score. The
//! [`Judge`] trait is the seam for a future LLM judge; [`RuleJudge`] is the
//! default and the only judge in the CI path.
//!
//! The `LlmJudge` implementation lives behind the `llm-judge` cargo feature
//! (default OFF). The feature is not declared in `Cargo.toml` (which this
//! task must not edit); enabling it with `--features llm-judge` still works,
//! and the `unexpected_cfgs` warning is expected in the default build.

#![allow(unexpected_cfgs)]

use crate::corpus::Case;
use crate::wer::tokenize;

/// Filler words/phrases that must not survive polishing. Matched
/// case-insensitively on word boundaries.
/// Single-word fillers only. Multi-word fillers (`you know`, `i mean`,
/// `sort of`, `kind of`) are excluded: their component words are ordinary
/// content words that legitimately appear in normal speech, and matching
/// them by word-boundary substring would flag correct output. The
/// single-word set is the safe, unambiguous half of the filler list.
const FILLERS: &[&str] = &["um", "uh", "er", "ah", "like", "basically", "actually"];

/// Stopword list for the content-word checks. Content words are the ones
/// that carry meaning; function words are allowed to be dropped or reworded
/// without counting as content loss.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "so", "if", "then", "of", "in", "on", "at", "to", "for",
    "with", "by", "from", "up", "about", "into", "over", "after", "before", "is", "are", "was",
    "were", "be", "been", "being", "am", "do", "does", "did",
];

/// Explicitly-allowed transformations: a reference word counts as present in
/// the output if it maps to an allowed variant (e.g. number words to digits,
/// `east one` -> `east-1`). This is the same set of rewrites a real polish
/// model (ITN) would apply, so the rubric does not punish correct normalisation.
const TRANSFORMS: &[(&str, &str)] = &[
    ("one", "1"),
    ("two", "2"),
    ("three", "3"),
    ("four", "4"),
    ("five", "5"),
    ("six", "6"),
    ("seven", "7"),
    ("eight", "8"),
    ("nine", "9"),
    ("ten", "10"),
    ("eleven", "11"),
    ("twelve", "12"),
    ("thirteen", "13"),
    ("fourteen", "14"),
    ("fifteen", "15"),
    ("sixteen", "16"),
    ("seventeen", "17"),
    ("eighteen", "18"),
    ("nineteen", "19"),
    ("twenty", "20"),
    ("thirty", "30"),
    ("forty", "40"),
    ("fifty", "50"),
    ("sixty", "60"),
    ("seventy", "70"),
    ("eighty", "80"),
    ("ninety", "90"),
    ("hundred", "100"),
    ("thousand", "1000"),
    ("million", "1000000"),
    ("fourteenth", "14th"),
];

/// Multi-word transformations: a run of reference tokens counts as present
/// if the output contains the joined token (e.g. `east one` -> `east-1`).
const MULTI_TRANSFORMS: &[(&[&str], &str)] = &[
    (&["east", "one"], "east-1"),
    (&["east", "two"], "east-2"),
    (&["west", "one"], "west-1"),
    (&["west", "two"], "west-2"),
];

/// One per-check result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
}

/// The result of scoring one polished string against its reference.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolishVerdict {
    pub checks: Vec<Check>,
    /// 0-100 weighted score.
    pub score: u32,
    /// Names of the checks that failed, for the report.
    pub failed: Vec<String>,
}

impl PolishVerdict {
    pub fn all_passed(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Weights sum to 100. `no_content_loss` and `no_hallucination` dominate
/// (50 + 40 = 90 of 100 points) because they are *correctness*: a polished
/// string that drops or invents content is wrong regardless of how clean it
/// looks. The remaining checks are surface polish.
const WEIGHTS: &[(&str, u32)] = &[
    ("no_content_loss", 50),
    ("no_hallucination", 40),
    ("no_fillers", 4),
    ("sentence_cased", 3),
    ("terminal_punctuation", 2),
    ("no_markdown_fences", 1),
    ("bullets_when_listing", 0), // structural; reported but not scored
];

/// Score `polished` against `reference` with the deterministic rubric.
pub fn score_polish(reference: &str, polished: &str) -> PolishVerdict {
    let mut checks = Vec::new();

    checks.push(Check {
        name: "no_fillers".into(),
        passed: check_no_fillers(polished),
    });
    checks.push(Check {
        name: "sentence_cased".into(),
        passed: check_sentence_cased(polished),
    });
    checks.push(Check {
        name: "terminal_punctuation".into(),
        passed: check_terminal_punctuation(polished),
    });

    let ref_content = content_words(reference);
    let out_content = content_words(polished);
    let out_all: Vec<String> = tokenize(polished);
    let ref_all: Vec<String> = tokenize(reference);

    checks.push(Check {
        name: "no_content_loss".into(),
        passed: check_no_content_loss(&ref_content, &out_content, &out_all),
    });
    checks.push(Check {
        name: "no_hallucination".into(),
        passed: check_no_hallucination(&out_content, &ref_content, &ref_all),
    });

    checks.push(Check {
        name: "no_markdown_fences".into(),
        passed: check_no_markdown_fences(polished),
    });
    checks.push(Check {
        name: "bullets_when_listing".into(),
        passed: check_bullets_when_listing(reference, polished),
    });

    let failed: Vec<String> = checks
        .iter()
        .filter(|c| !c.passed)
        .map(|c| c.name.clone())
        .collect();

    let score: u32 = checks
        .iter()
        .filter(|c| c.passed)
        .map(|c| {
            WEIGHTS
                .iter()
                .find(|(name, _)| *name == c.name)
                .map(|(_, w)| *w)
                .unwrap_or(0)
        })
        .sum();

    PolishVerdict {
        checks,
        score,
        failed,
    }
}

/// Content words of `text`: WER-normalised tokens minus stopwords.
fn content_words(text: &str) -> Vec<String> {
    tokenize(text)
        .into_iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// A reference token is "present" in the output if it appears verbatim, or
/// maps to an allowed transformation that appears in the output.
fn is_present(ref_word: &str, out_content: &[String], out_all: &[String]) -> bool {
    if out_content.iter().any(|w| w == ref_word) {
        return true;
    }
    TRANSFORMS
        .iter()
        .find(|(from, _)| *from == ref_word)
        .is_some_and(|(_, to)| out_all.iter().any(|w| w == to))
}

fn check_no_content_loss(
    ref_content: &[String],
    out_content: &[String],
    out_all: &[String],
) -> bool {
    ref_content
        .iter()
        .all(|w| is_present(w, out_content, out_all))
}

fn check_no_hallucination(
    out_content: &[String],
    ref_content: &[String],
    ref_all: &[String],
) -> bool {
    // A content word in the output is a hallucination unless it appears in
    // the reference (as any token) or is an allowed transformation of a
    // reference word (single- or multi-word).
    out_content.iter().all(|w| {
        if ref_all.iter().any(|r| r == w) {
            return true;
        }
        let single = TRANSFORMS
            .iter()
            .any(|(from, to)| to == w && ref_content.iter().any(|r| r == from));
        if single {
            return true;
        }
        // Multi-word: the output token is the joined form of a reference run.
        MULTI_TRANSFORMS.iter().any(|(from, to)| {
            to == w
                && (0..=ref_all.len().saturating_sub(from.len()))
                    .any(|i| from.iter().enumerate().all(|(k, fw)| ref_all[i + k] == *fw))
        })
    })
}

fn check_no_fillers(polished: &str) -> bool {
    let lowered = polished.to_lowercase();
    FILLERS.iter().all(|f| !contains_word(&lowered, f))
}

/// Whole-word match at word boundaries. Punctuation glued to a word
/// ("um,", "er.") does not hide it: a token matches when its alphabetic
/// core equals the needle. Multi-word fillers match as consecutive tokens.
fn contains_word(haystack: &str, needle: &str) -> bool {
    fn core(token: &str) -> String {
        token.chars().filter(|c| c.is_alphanumeric()).collect()
    }
    let h: Vec<String> = haystack
        .split_whitespace()
        .map(core)
        .filter(|c| !c.is_empty())
        .collect();
    let n: Vec<&str> = needle.split_whitespace().collect();
    if n.is_empty() {
        return false;
    }
    (0..=h.len().saturating_sub(n.len()))
        .any(|i| h[i..i + n.len()].iter().zip(n.iter()).all(|(a, b)| a == *b))
}

fn check_sentence_cased(polished: &str) -> bool {
    polished
        .chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(|c| c.is_uppercase())
}

fn check_terminal_punctuation(polished: &str) -> bool {
    polished
        .trim_end()
        .chars()
        .next_back()
        .is_some_and(|c| matches!(c, '.' | '!' | '?'))
}

fn check_no_markdown_fences(polished: &str) -> bool {
    if polished.contains("```") {
        return false;
    }
    let first_line = polished.lines().next().unwrap_or("");
    !first_line.trim().to_lowercase().starts_with("here is")
        && !first_line.trim().to_lowercase().starts_with("here's")
}

/// If the reference announces a list, the output must render one `- ` item
/// per line, each ending in a period. Non-list references pass trivially.
fn check_bullets_when_listing(reference: &str, polished: &str) -> bool {
    if !announces_list(reference) {
        return true;
    }
    let lines: Vec<&str> = polished
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return false;
    }
    // The lead-in line (the part before the items, e.g. "Buy groceries:")
    // is not a bullet; only the `- ` items must each end in a period.
    lines
        .iter()
        .filter(|l| l.starts_with("- ") || l.starts_with("-"))
        .all(|l| l.trim_end().ends_with('.'))
        && lines
            .iter()
            .any(|l| l.starts_with("- ") || l.starts_with("-"))
}

/// A reference announces a list when it has a colon after a lead-in phrase,
/// or uses first/second/third ordinals followed by a separator.
fn announces_list(reference: &str) -> bool {
    let lowered = reference.to_lowercase();
    // Colon after a lead-in: "things to do:", "the steps are:" etc.
    if lowered.contains(':') {
        return true;
    }
    let has_ordinal = ["first", "second", "third"].iter().any(|o| {
        lowered
            .split_whitespace()
            .any(|w| w == *o || w == format!("{o},") || w == format!("{o};"))
    });
    has_ordinal && lowered.contains([';', ',', ':'])
}

/// A judge scores polished output. The default is [`RuleJudge`]; an LLM
/// judge can be added behind the `llm-judge` feature without touching the
/// report code.
pub trait Judge {
    fn name(&self) -> &str;
    fn judge(&self, case: &Case, polished: &str) -> Result<PolishVerdict, String>;
}

/// The deterministic rubric judge (the default).
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleJudge;

impl Judge for RuleJudge {
    fn name(&self) -> &str {
        "rule"
    }

    fn judge(&self, case: &Case, polished: &str) -> Result<PolishVerdict, String> {
        Ok(score_polish(&case.reference, polished))
    }
}

#[cfg(feature = "llm-judge")]
mod llm {
    use super::*;

    /// An LLM judge that POSTs to an OpenAI-compatible
    /// `/v1/chat/completions` endpoint. All configuration comes from
    /// environment variables, never from a committed file:
    ///
    /// - `TELETYPE_EVAL_LLM_URL` — full endpoint URL
    ///   (e.g. `https://api.openai.com/v1/chat/completions`)
    /// - `TELETYPE_EVAL_LLM_API_KEY` — bearer token
    /// - `TELETYPE_EVAL_LLM_MODEL` — model id (e.g. `gpt-4o-mini`)
    #[derive(Debug, Clone)]
    pub struct LlmJudge {
        url: String,
        api_key: String,
        model: String,
    }

    impl LlmJudge {
        pub fn from_env() -> Result<Self, String> {
            let url = std::env::var("TELETYPE_EVAL_LLM_URL")
                .map_err(|_| "TELETYPE_EVAL_LLM_URL is not set".to_string())?;
            let api_key = std::env::var("TELETYPE_EVAL_LLM_API_KEY")
                .map_err(|_| "TELETYPE_EVAL_LLM_API_KEY is not set".to_string())?;
            let model = std::env::var("TELETYPE_EVAL_LLM_MODEL")
                .map_err(|_| "TELETYPE_EVAL_LLM_MODEL is not set".to_string())?;
            Ok(Self {
                url,
                api_key,
                model,
            })
        }
    }

    impl Judge for LlmJudge {
        fn name(&self) -> &str {
            "llm"
        }

        fn judge(&self, case: &Case, polished: &str) -> Result<PolishVerdict, String> {
            let system = "You are a strict editor. Judge the polished text against the reference. \
                          Return ONLY a JSON object: {\"score\": <0-100>, \"checks\": \
                          [{\"name\": \"<check>\", \"passed\": <bool>}], \"failed\": [\"<check>\"]}.\n\
                          Checks: no_fillers, sentence_cased, terminal_punctuation, \
                          no_content_loss, no_hallucination, no_markdown_fences, \
                          bullets_when_listing.";
            let user = format!("Reference: {}\nPolished: {}\n", case.reference, polished);
            let body = serde_json::json!({
                "model": self.model,
                "temperature": 0,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            });

            let raw = post_chat(&self.url, &self.api_key, &body.to_string())?;
            let value: serde_json::Value =
                serde_json::from_str(&raw).map_err(|e| format!("bad JSON from LLM: {e}"))?;
            let content = value["choices"][0]["message"]["content"]
                .as_str()
                .ok_or("no content in LLM response")?;
            // Tolerate a code fence around the JSON.
            let content = content
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            let v: serde_json::Value =
                serde_json::from_str(content).map_err(|e| format!("bad verdict JSON: {e}"))?;
            let score = v["score"].as_u64().unwrap_or(0).min(100) as u32;
            let checks: Vec<Check> = v["checks"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            Some(Check {
                                name: c["name"].as_str()?,
                                passed: c["passed"].as_bool()?,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let failed: Vec<&'static str> = checks
                .iter()
                .filter(|c| !c.passed)
                .map(|c| c.name)
                .collect();
            Ok(PolishVerdict {
                checks,
                score,
                failed,
            })
        }
    }

    /// Minimal blocking HTTP POST. This crate intentionally has no HTTP
    /// dependency in the default build; the `llm-judge` feature is opt-in and
    /// CI never enables it.
    fn post_chat(url: &str, api_key: &str, body: &str) -> Result<String, String> {
        let host = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .ok_or("URL must be http(s)")?;
        let (host, path) = host
            .split_once('/')
            .map(|(h, p)| (h, format!("/{p}")))
            .unwrap_or((host, "/".to_string()));
        let is_https = url.starts_with("https://");

        let sock = std::net::TcpStream::connect((host, if is_https { 443 } else { 80 }))
            .map_err(|e| format!("connect {host}: {e}"))?;
        let mut sock = sock;
        if is_https {
            return Err(
                "LlmJudge requires TLS; this minimal implementation does not \
                 bundle a TLS stack. Point TELETYPE_EVAL_LLM_URL at a local \
                 http:// endpoint (e.g. llama-server) instead."
                    .to_string(),
            );
        }
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {api_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        use std::io::{Read, Write};
        sock.write_all(req.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        let mut raw = Vec::new();
        sock.read_to_end(&mut raw)
            .map_err(|e| format!("read: {e}"))?;
        let text = String::from_utf8_lossy(&raw).to_string();
        // Split headers from body.
        let body_start = text.find("\r\n\r\n").map(|i| i + 4);
        match body_start {
            Some(i) => Ok(text[i..].to_string()),
            None => Err("no HTTP response body".into()),
        }
    }
}

#[cfg(feature = "llm-judge")]
pub use llm::LlmJudge;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn case(reference: &str) -> Case {
        Case {
            id: "t".into(),
            audio: PathBuf::from("t.wav"),
            audio_path: PathBuf::from("t.wav"),
            reference: reference.into(),
            tags: vec![],
            human: false,
        }
    }

    #[test]
    fn perfect_polish_scores_100() {
        let v = score_polish(
            "the deploy went out this morning and it is all good",
            "The deploy went out this morning, and it is all good.",
        );
        assert_eq!(v.score, 100, "failed: {:?}", v.failed);
        assert!(v.all_passed());
    }

    #[test]
    fn fillers_are_flagged() {
        // "um" is a single-word filler, not a stopword, and absent from the
        // reference: it must trip both no_fillers and no_hallucination.
        let v = score_polish(
            "the deploy went out this morning",
            "Um, the deploy went out this morning.",
        );
        assert!(
            v.failed.contains(&"no_fillers".to_string()),
            "{:?}",
            v.failed
        );
        assert!(
            v.failed.contains(&"no_hallucination".to_string()),
            "um is not in the reference and not stopword-exempt: {:?}",
            v.failed
        );
    }

    #[test]
    fn lowercase_start_fails_sentence_cased() {
        let v = score_polish("hello world", "hello world.");
        assert!(v.failed.contains(&"sentence_cased".to_string()));
    }

    #[test]
    fn no_terminal_punctuation_fails() {
        let v = score_polish("hello world", "Hello world");
        assert!(v.failed.contains(&"terminal_punctuation".to_string()));
    }

    #[test]
    fn content_loss_is_flagged() {
        // "deploy" is a content word in the reference but missing from the output.
        let v = score_polish("the deploy went out", "It went out.");
        assert!(v.failed.contains(&"no_content_loss".to_string()));
    }

    #[test]
    fn hallucination_is_flagged() {
        // "rocket" is a content word in the output but absent from the reference.
        let v = score_polish("the deploy went out", "The deploy went out on a rocket.");
        assert!(v.failed.contains(&"no_hallucination".to_string()));
    }

    #[test]
    fn number_word_to_digit_is_allowed() {
        // "three" -> "3" counts as present (no content loss, no hallucination).
        let v = score_polish("we have three servers", "We have 3 servers.");
        assert!(
            !v.failed.contains(&"no_content_loss".to_string()),
            "{:?}",
            v.failed
        );
        assert!(
            !v.failed.contains(&"no_hallucination".to_string()),
            "{:?}",
            v.failed
        );
    }

    #[test]
    fn markdown_fence_is_flagged() {
        let v = score_polish("hello world", "Here is the text:\n```\nHello world.\n```");
        assert!(v.failed.contains(&"no_markdown_fences".to_string()));
    }

    #[test]
    fn here_is_preamble_is_flagged() {
        let v = score_polish("hello world", "Here is your text: Hello world.");
        assert!(v.failed.contains(&"no_markdown_fences".to_string()));
    }

    #[test]
    fn list_without_bullets_fails() {
        let v = score_polish(
            "buy groceries: eggs, milk, bread",
            "Buy groceries: eggs, milk, bread.",
        );
        assert!(v.failed.contains(&"bullets_when_listing".to_string()));
    }

    #[test]
    fn list_with_bullets_passes() {
        let v = score_polish(
            "buy groceries: eggs, milk, bread",
            "Buy groceries:\n- Eggs.\n- Milk.\n- Bread.",
        );
        assert!(
            !v.failed.contains(&"bullets_when_listing".to_string()),
            "{:?}",
            v.failed
        );
    }

    #[test]
    fn non_list_reference_passes_trivially() {
        let v = score_polish("the cat sat on the mat", "The cat sat on the mat.");
        assert!(!v.failed.contains(&"bullets_when_listing".to_string()));
    }

    #[test]
    fn raw_passthrough_fails_polish_checks() {
        // If the transform fell back to the raw input (no casing, no
        // punctuation), the rubric must reflect that.
        let v = score_polish("the cat sat on the mat", "the cat sat on the mat");
        assert!(v.failed.contains(&"sentence_cased".to_string()));
        assert!(v.failed.contains(&"terminal_punctuation".to_string()));
        // Content checks still pass (nothing was lost or invented).
        assert!(!v.failed.contains(&"no_content_loss".to_string()));
        assert!(!v.failed.contains(&"no_hallucination".to_string()));
    }

    #[test]
    fn rule_judge_matches_direct_scoring() {
        let c = case("the cat sat on the mat");
        let v = RuleJudge {}.judge(&c, "The cat sat on the mat.").unwrap();
        assert_eq!(v.score, 100);
        assert_eq!(RuleJudge.name(), "rule");
    }

    #[test]
    fn score_is_bounded() {
        let v = score_polish("alpha beta gamma", "totally different text!");
        assert!(v.score <= 100);
        // "alpha", "beta", "gamma" all lost; "totally", "different", "text" all hallucinated.
        assert!(v.score < 50);
    }

    #[test]
    fn weights_dominate_on_correctness() {
        // A string that passes all surface checks but loses one content word
        // must score far below one that keeps content but has a minor
        // casing issue.
        let lossy = score_polish("the deploy went out", "It went out.");
        let minor = score_polish("the deploy went out", "The deploy went out");
        // lossy: fails no_content_loss (50) + terminal_punctuation (2) +
        //        no_hallucination? "it" is a stopword, "went" "out" are in ref.
        //        Actually "it" is in STOPWORDS so no hallucination.
        //        Score = 100 - 50 - 2 = 48.
        // minor: fails terminal_punctuation (2). Score = 98.
        assert!(
            minor.score > lossy.score,
            "content loss (score {}) must dominate surface polish (score {})",
            lossy.score,
            minor.score
        );
    }
}
