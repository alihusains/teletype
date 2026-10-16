//! Parse the model's decoded output into [`ToolCall`]s.
//!
//! The model is trained to emit a JSON list of tool call objects:
//! `[{"name": "open_app", "arguments": {"app": "terminal"}}, ...]`
//!
//! This parser is strict: the entire decoded string must be a valid JSON
//! array of objects with a `"name"` string field and an optional
//! `"arguments"` object. It never panics on arbitrary input (proptest-
//! verified).

use crate::error::{DeciderError, Result};
use crate::tool::ToolCall;
use serde_json::Value;

/// Parse a decoded model output string into a list of [`ToolCall`]s.
///
/// # Errors
///
/// Returns [`DeciderError::Parse`] if the input is not a valid JSON array
/// of tool call objects. Never panics.
pub fn parse_tool_calls(decoded: &str) -> Result<Vec<ToolCall>> {
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        return Err(DeciderError::Parse("empty input".into()));
    }

    let value: Value = serde_json::from_str(trimmed)
        .map_err(|e| DeciderError::Parse(format!("not valid JSON: {e}")))?;

    let arr = value
        .as_array()
        .ok_or_else(|| DeciderError::Parse("expected a JSON array".into()))?;

    let mut calls = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| DeciderError::Parse(format!("item {i}: expected an object")))?;

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                DeciderError::Parse(format!("item {i}: missing or non-string \"name\""))
            })?
            .to_string();

        let arguments = match obj.get("arguments") {
            None => serde_json::Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(other) => {
                return Err(DeciderError::Parse(format!(
                    "item {i}: \"arguments\" must be an object, got {other:?}"
                )))
            }
        };

        calls.push(ToolCall { name, arguments });
    }

    Ok(calls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_call() {
        let calls = parse_tool_calls(r#"[{"name": "open_app", "arguments": {"app": "terminal"}}]"#)
            .unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "open_app");
        assert_eq!(calls[0].arguments.get("app").unwrap(), "terminal");
    }

    #[test]
    fn parse_multiple_calls() {
        let calls = parse_tool_calls(
            r#"[{"name": "no_action", "arguments": {}}, {"name": "web_search", "arguments": {"query": "rust"}}]"#,
        )
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "no_action");
        assert!(calls[0].arguments.is_empty());
        assert_eq!(calls[1].name, "web_search");
    }

    #[test]
    fn parse_missing_arguments() {
        let calls = parse_tool_calls(r#"[{"name": "paste_last"}]"#).unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].arguments.is_empty());
    }

    #[test]
    fn parse_empty_array() {
        let calls = parse_tool_calls("[]").unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn reject_non_array() {
        assert!(parse_tool_calls(r#"{"name": "no_action"}"#).is_err());
        assert!(parse_tool_calls(r#""hello""#).is_err());
        assert!(parse_tool_calls("42").is_err());
    }

    #[test]
    fn reject_non_object_items() {
        assert!(parse_tool_calls(r#"[1, 2, 3]"#).is_err());
        assert!(parse_tool_calls(r#"[["nested"]]"#).is_err());
    }

    #[test]
    fn reject_missing_name() {
        assert!(parse_tool_calls(r#"[{"arguments": {}}]"#).is_err());
    }

    #[test]
    fn reject_non_string_name() {
        assert!(parse_tool_calls(r#"[{"name": 42}]"#).is_err());
    }

    #[test]
    fn reject_non_object_arguments() {
        assert!(parse_tool_calls(r#"[{"name": "x", "arguments": "bad"}]"#).is_err());
        assert!(parse_tool_calls(r#"[{"name": "x", "arguments": [1]}]"#).is_err());
    }

    #[test]
    fn reject_empty_input() {
        assert!(parse_tool_calls("").is_err());
        assert!(parse_tool_calls("   ").is_err());
    }

    #[test]
    fn reject_invalid_json() {
        assert!(parse_tool_calls("[{").is_err());
        assert!(parse_tool_calls("not json at all").is_err());
    }

    #[test]
    fn trailing_whitespace_ok() {
        let calls = parse_tool_calls(r#"[{"name": "no_action", "arguments": {}}]   "#).unwrap();
        assert_eq!(calls.len(), 1);
    }
}
