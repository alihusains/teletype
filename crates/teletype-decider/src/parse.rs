//! Parse the model's decoded output into [`ToolCall`]s.
//!
//! The model is trained to emit a JSON list of tool call objects:
//! `[{"name": "open_app", "arguments": {"app": "terminal"}}, ...]`
//!
//! The parser accepts the first top-level JSON array and ignores any
//! trailing tokens the model appends after it (e.g. an EOS token). It
//! never panics on arbitrary input (proptest-verified).
//!
//! Decoding noise or silence can produce non-JSON garbage; that is a
//! "no command" outcome, not an error. [`parse_tool_calls`] therefore
//! returns an empty list for any unparseable input instead of erroring.

use crate::tool::ToolCall;
use serde_json::Value;

/// Parse a decoded model output string into a list of [`ToolCall`]s.
///
/// The first top-level JSON array in the input wins; any trailing tokens
/// the model appends after it (e.g. an EOS token) are ignored.
///
/// Noise and silence can make the model emit non-JSON garbage, which is a
/// "no command" outcome rather than an error: any unparseable input
/// (including the empty string) yields an empty list. Never panics.
pub fn parse_tool_calls(decoded: &str) -> Vec<ToolCall> {
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // The model is trained to emit a JSON array, but may append trailing
    // tokens (e.g. EOS or a second line). Find the end of the first array
    // and parse up to that point.
    let value: Value = match serde_json::from_str::<Value>(trimmed) {
        Ok(v) => v,
        Err(_) => {
            let Some(end) = find_array_end(trimmed) else {
                return Vec::new();
            };
            match serde_json::from_str(&trimmed[..end]) {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            }
        }
    };

    let Some(arr) = value.as_array() else {
        return Vec::new();
    };

    let mut calls = Vec::with_capacity(arr.len());
    for item in arr.iter() {
        let Some(obj) = item.as_object() else {
            return Vec::new();
        };

        let Some(name) = obj.get("name").and_then(|v| v.as_str()) else {
            return Vec::new();
        };

        let arguments = match obj.get("arguments") {
            None => serde_json::Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(_) => return Vec::new(),
        };

        calls.push(ToolCall {
            name: name.to_string(),
            arguments,
        });
    }

    calls
}

/// Find the byte index just past the first top-level JSON array in `s`.
///
/// Returns `None` if the string does not start with `[` or if the array
/// is not closed.
fn find_array_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' => depth += 1,
            b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_call() {
        let calls = parse_tool_calls(r#"[{"name": "open_app", "arguments": {"app": "terminal"}}]"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "open_app");
        assert_eq!(calls[0].arguments.get("app").unwrap(), "terminal");
    }

    #[test]
    fn parse_multiple_calls() {
        let calls = parse_tool_calls(
            r#"[{"name": "no_action", "arguments": {}}, {"name": "web_search", "arguments": {"query": "rust"}}]"#,
        );
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "no_action");
        assert!(calls[0].arguments.is_empty());
        assert_eq!(calls[1].name, "web_search");
    }

    #[test]
    fn parse_missing_arguments() {
        let calls = parse_tool_calls(r#"[{"name": "paste_last"}]"#);
        assert_eq!(calls.len(), 1);
        assert!(calls[0].arguments.is_empty());
    }

    #[test]
    fn parse_empty_array() {
        let calls = parse_tool_calls("[]");
        assert!(calls.is_empty());
    }

    #[test]
    fn non_array_yields_empty() {
        assert!(parse_tool_calls(r#"{"name": "no_action"}"#).is_empty());
        assert!(parse_tool_calls(r#""hello""#).is_empty());
        assert!(parse_tool_calls("42").is_empty());
    }

    #[test]
    fn non_object_items_yield_empty() {
        assert!(parse_tool_calls(r#"[1, 2, 3]"#).is_empty());
        assert!(parse_tool_calls(r#"[["nested"]]"#).is_empty());
    }

    #[test]
    fn missing_name_yields_empty() {
        assert!(parse_tool_calls(r#"[{"arguments": {}}]"#).is_empty());
    }

    #[test]
    fn non_string_name_yields_empty() {
        assert!(parse_tool_calls(r#"[{"name": 42}]"#).is_empty());
    }

    #[test]
    fn non_object_arguments_yield_empty() {
        assert!(parse_tool_calls(r#"[{"name": "x", "arguments": "bad"}]"#).is_empty());
        assert!(parse_tool_calls(r#"[{"name": "x", "arguments": [1]}]"#).is_empty());
    }

    #[test]
    fn empty_input_yields_empty() {
        assert!(parse_tool_calls("").is_empty());
        assert!(parse_tool_calls("   ").is_empty());
    }

    #[test]
    fn invalid_json_yields_empty() {
        assert!(parse_tool_calls("[{").is_empty());
        assert!(parse_tool_calls("not json at all").is_empty());
    }

    #[test]
    fn noise_garbage_yields_empty() {
        // What the model actually emits on silence/noise (observed):
        // unstructured token soup, no JSON at all.
        let garbage = "\" q ps q q q ( \" q ps q q q q ( \" q ps q q q q q q ps q q";
        assert!(parse_tool_calls(garbage).is_empty());
    }

    #[test]
    fn trailing_whitespace_ok() {
        let calls = parse_tool_calls(r#"[{"name": "no_action", "arguments": {}}]   "#);
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn trailing_tokens_after_array_ignored() {
        let calls = parse_tool_calls(r#"[{"name": "no_action", "arguments": {}}] trailing"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "no_action");
    }

    #[test]
    fn trailing_tokens_with_brackets_in_strings() {
        let calls = parse_tool_calls(
            r#"[{"name": "web_search", "arguments": {"query": "a]b[c"}}] trailing"#,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments.get("query").unwrap(), "a]b[c");
    }

    #[test]
    fn trailing_tokens_unbalanced_yields_empty() {
        // No closing bracket: find_array_end returns None, so the input
        // is unparseable and the result is an empty list (no trigger).
        assert!(parse_tool_calls(r#"[{"name": "x"} trailing"#).is_empty());
    }
}
