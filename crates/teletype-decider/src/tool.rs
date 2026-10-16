//! Tool-call type shared by the parser and the runtime.

/// One decoded tool call: a tool name and its JSON arguments.
///
/// `arguments` preserves the key order the model emitted (serde_json in this
/// crate is built with `preserve_order`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    /// The tool name, e.g. `"open_app"`.
    pub name: String,
    /// The arguments object. Empty for tools without parameters.
    #[serde(default)]
    pub arguments: serde_json::Map<String, serde_json::Value>,
}
