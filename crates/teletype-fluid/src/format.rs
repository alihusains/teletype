//! Wire-format helpers shared by the real bridge and its stub: the JSON the
//! Swift side returns, the JSON we send it, and the buffer sizing rules.
//!
//! Kept platform-neutral so the parsing rules are unit-tested on every
//! platform, not only where the Swift library is linked.

use teletype_speech::{BoostTerm, SpeechError, Transcript, Word};

/// Smallest output buffer we hand the bridge for a transcript.
pub(crate) const MIN_OUT_CAPACITY: usize = 16 * 1024;

/// Refuse absurd retry sizes (a runaway bridge should fail loudly, not OOM).
pub(crate) const MAX_OUT_CAPACITY: usize = 8 * 1024 * 1024;

/// Initial output capacity for `sample_count` f32 samples. Transcript JSON
/// carries the text plus per-word and per-token timings (~4 bytes of JSON per
/// second of 16 kHz audio, with generous slack), so this is far above real
/// need and the retry path is a safety net, not the common case.
pub(crate) fn initial_out_capacity(sample_count: usize) -> usize {
    (4096 + sample_count / 4).clamp(MIN_OUT_CAPACITY, MAX_OUT_CAPACITY)
}

/// Decode a NUL-terminated UTF-8 buffer from the bridge (lossy on invalid
/// bytes; everything after the first NUL is ignored).
pub(crate) fn decode_buffer(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// A parsed transcript plus the vocabulary-boost telemetry the bridge reports.
#[derive(Debug)]
pub(crate) struct ParsedTranscript {
    pub transcript: Transcript,
    pub detected_terms: Vec<String>,
    pub applied_terms: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawTranscript {
    text: String,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    words: Vec<RawWord>,
    #[serde(default)]
    detected_terms: Vec<String>,
    #[serde(default)]
    applied_terms: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawWord {
    text: String,
    start_ms: u32,
    end_ms: u32,
    conf: f32,
}

/// Parse the bridge's transcript JSON into the shared [`Transcript`] shape.
///
/// `language` is always `None`: FluidAudio does not report a detected
/// language, and the caller-supplied hint (when any) is a lock, not a
/// detection.
pub(crate) fn parse_transcript_json(json: &str) -> Result<ParsedTranscript, SpeechError> {
    let raw: RawTranscript = serde_json::from_str(json).map_err(|error| {
        SpeechError::Transcribe(format!("FluidAudio returned unparsable JSON: {error}"))
    })?;
    let words = raw
        .words
        .into_iter()
        .filter(|word| !word.text.is_empty())
        .map(|word| Word {
            text: word.text,
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            conf: word.conf,
        })
        .collect();
    Ok(ParsedTranscript {
        transcript: Transcript {
            text: raw.text,
            words,
            confidence: raw.confidence,
            language: None,
        },
        detected_terms: raw.detected_terms,
        applied_terms: raw.applied_terms,
    })
}

/// Serialize boost terms for `ttf_set_vocab`. Non-finite weights are
/// normalized to 1.0 first so serialization cannot fail.
pub(crate) fn boost_terms_json(terms: &[BoostTerm]) -> String {
    #[derive(serde::Serialize)]
    struct RawTerm<'a> {
        text: &'a str,
        aliases: &'a [String],
        weight: f32,
    }

    let raw: Vec<RawTerm<'_>> = terms
        .iter()
        .map(|term| RawTerm {
            text: &term.text,
            aliases: &term.aliases,
            weight: if term.weight.is_finite() {
                term.weight
            } else {
                1.0
            },
        })
        .collect();
    serde_json::to_string(&raw).expect("finite floats and strings always serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> &'static str {
        r#"{"confidence":0.96,"duration_ms":1200,
            "text":"Hello world.",
            "tokens":[{"token":"\u2581Hello","start_ms":0,"end_ms":300,"conf":0.97},
                      {"token":"\u2581world","start_ms":320,"end_ms":600,"conf":0.94}],
            "words":[{"text":"Hello","start_ms":0,"end_ms":300,"conf":0.97},
                     {"text":"world.","start_ms":320,"end_ms":600,"conf":0.94}],
            "detected_terms":["Teletype"],"applied_terms":["Teletype"]}"#
    }

    #[test]
    fn parses_a_detailed_transcript() {
        let parsed = parse_transcript_json(sample_json()).expect("fixture must parse");
        assert_eq!(parsed.transcript.text, "Hello world.");
        assert_eq!(parsed.transcript.words.len(), 2);
        assert_eq!(parsed.transcript.words[1].text, "world.");
        assert_eq!(parsed.transcript.words[1].start_ms, 320);
        assert_eq!(parsed.transcript.words[1].end_ms, 600);
        assert!((parsed.transcript.confidence - 0.96).abs() < 1e-6);
        // FluidAudio never reports a detected language.
        assert_eq!(parsed.transcript.language, None);
        assert_eq!(parsed.detected_terms, vec!["Teletype".to_string()]);
        assert_eq!(parsed.applied_terms, vec!["Teletype".to_string()]);
    }

    #[test]
    fn missing_optional_sections_default_to_empty() {
        let parsed = parse_transcript_json(r#"{"text":"hi"}"#).expect("minimal payload must parse");
        assert_eq!(parsed.transcript.text, "hi");
        assert!(parsed.transcript.words.is_empty());
        assert_eq!(parsed.transcript.confidence, 0.0);
        assert!(parsed.detected_terms.is_empty());
    }

    #[test]
    fn garbage_json_is_a_transcribe_error() {
        let error = parse_transcript_json("not json").expect_err("must fail");
        match error {
            SpeechError::Transcribe(message) => {
                assert!(message.contains("unparsable JSON"), "got: {message}");
            }
            other => panic!("wrong error variant: {other:?}"),
        }
    }

    #[test]
    fn empty_word_text_is_dropped() {
        let json = r#"{"text":"x","words":[{"text":"","start_ms":0,"end_ms":0,"conf":0.0},
                                            {"text":"x","start_ms":0,"end_ms":50,"conf":0.9}]}"#;
        let parsed = parse_transcript_json(json).expect("parse");
        assert_eq!(parsed.transcript.words.len(), 1);
        assert_eq!(parsed.transcript.words[0].text, "x");
    }

    #[test]
    fn boost_terms_serialize_to_the_bridge_shape() {
        let terms = vec![
            BoostTerm {
                text: "Siobhan".into(),
                aliases: vec!["shivon".into()],
                weight: 1.5,
            },
            BoostTerm {
                text: "Teletype".into(),
                aliases: vec![],
                weight: 1.0,
            },
        ];
        let json = boost_terms_json(&terms);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            parsed,
            serde_json::json!([
                {"text": "Siobhan", "aliases": ["shivon"], "weight": 1.5},
                {"text": "Teletype", "aliases": [], "weight": 1.0},
            ])
        );
    }

    #[test]
    fn non_finite_boost_weight_is_normalized() {
        let terms = vec![BoostTerm {
            text: "NaN".into(),
            aliases: vec![],
            weight: f32::NAN,
        }];
        let parsed: serde_json::Value =
            serde_json::from_str(&boost_terms_json(&terms)).expect("serializes");
        assert_eq!(parsed[0]["weight"], serde_json::json!(1.0));
    }

    #[test]
    fn decode_buffer_stops_at_the_nul() {
        let buffer = b"hello\0world";
        assert_eq!(decode_buffer(buffer), "hello");
        // Missing NUL is tolerated: the whole slice is decoded.
        assert_eq!(decode_buffer(b"hello"), "hello");
    }

    #[test]
    fn initial_capacity_stays_within_bounds() {
        assert_eq!(initial_out_capacity(0), MIN_OUT_CAPACITY);
        assert_eq!(initial_out_capacity(10), MIN_OUT_CAPACITY);
        assert_eq!(initial_out_capacity(16_000 * 60), 4096 + 16_000 * 60 / 4);
        assert_eq!(initial_out_capacity(usize::MAX), MAX_OUT_CAPACITY);
    }
}
