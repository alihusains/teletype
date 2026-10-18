//! Python-`json.dumps`-compatible serialization.
//!
//! The NeuDecide model was trained on tool lists serialized with Python's
//! `json.dumps(tools, ensure_ascii=True)`. This module reproduces that
//! format exactly so the Rust crate can generate byte-identical input.
//!
//! Key differences from serde_json's default `to_string`:
//! - Separators are `", "` and `": "` (serde_json default: `","` / `":"`).
//! - Key order is preserved (serde_json `preserve_order` feature).
//! - All non-ASCII characters are escaped as `\uXXXX` (astral chars →
//!   surrogate pairs).
//! - `/` is never escaped (Python does not escape it).
//! - Floats use Python's `repr(float)` rules (shortest round-trip).

use serde_json::{Map, Value};

/// A tool definition as it appears in the serialized tool list.
///
/// `parameters` is a free-form JSON value: usually a JSON-Schema object
/// (`{"type":"object","properties":{...},"required":[...]}`) but the model
/// does not require that shape.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolDef {
    /// Tool name, e.g. `"open_app"`.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Parameter schema. `Some({})` serializes as an explicit empty object
    /// (`paste_last` / `no_action`); `None` omits the `parameters` key
    /// entirely (the model treats a missing key as empty — compat case 17).
    pub parameters: Option<Value>,
}

/// Serialize a `serde_json::Value` with Python `json.dumps` semantics:
/// separators `", "` / `": "`, `ensure_ascii=True`, key order preserved.
pub fn dump_python_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

fn write_value(out: &mut String, value: &Value, _depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_string(out, s),
        Value::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(out, item, _depth + 1);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (key, val)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_string(out, key);
                out.push_str(": ");
                write_value(out, val, _depth + 1);
            }
            out.push('}');
        }
    }
}

/// Write a JSON string with `ensure_ascii=True` escaping.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                // Other control chars (U+0000..U+001F) → \u00XX
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x7f => {
                // ASCII printable (0x20..0x7E) — never escaped
                out.push(c);
            }
            c => {
                // Non-ASCII → ensure_ascii: \uXXXX (surrogate pair for astral)
                let cp = c as u32;
                if cp <= 0xFFFF {
                    out.push_str(&format!("\\u{:04x}", cp));
                } else {
                    // Surrogate pair
                    let v = cp - 0x10000;
                    let hi = 0xD800 + (v >> 10);
                    let lo = 0xDC00 + (v & 0x3FF);
                    out.push_str(&format!("\\u{:04x}\\u{:04x}", hi, lo));
                }
            }
        }
    }
    out.push('"');
}

/// Write a JSON number with Python `json.dumps` semantics.
///
/// - Integers: plain decimal.
/// - Floats: Python `repr(float)` — shortest round-trip, `1.0` stays `1.0`,
///   exponent form for values < 1e-4 or >= 1e16.
fn write_number(out: &mut String, n: &serde_json::Number) {
    if let Some(i) = n.as_i64() {
        out.push_str(&i.to_string());
        return;
    }
    if let Some(u) = n.as_u64() {
        out.push_str(&u.to_string());
        return;
    }
    if let Some(f) = n.as_f64() {
        out.push_str(&format_python_float(f));
        return;
    }
    // Fallback (should not happen for valid serde_json numbers)
    out.push_str(&n.to_string());
}

/// Format an f64 the way Python's `json.dumps` would.
///
/// Python uses `repr(float)` which produces the shortest string that
/// round-trips. The key rules:
/// - Integer-valued floats get a trailing `.0`: `1.0`, `42.0`.
/// - Exponent form for `abs(x) < 1e-4` or `abs(x) >= 1e16`.
/// - Python uses `e+` / `e-` with at least one digit in the exponent:
///   `1e+16`, `1e-05`.
///
/// Rust's `{}` for f64 already produces shortest round-trip (Rust 1.59+),
/// so we only need to fix the integer-valued case and the exponent format.
///
/// # Remaining risk
///
/// Python's `repr` and Rust's shortest-float algorithm (Ryū / Grisu) can
/// differ in *which* shortest representation they pick when multiple
/// shortest strings round-trip. In practice this is extremely rare for
/// values that appear in tool JSON (typically small integers or simple
/// fractions). The fixtures avoid floats for this reason.
pub fn format_python_float(f: f64) -> String {
    if f.is_nan() || f.is_infinite() {
        // Python json.dumps rejects these by default; serde_json also
        // rejects them at parse time. This path is unreachable in practice.
        return String::from("null");
    }
    if f == f.trunc() && f.abs() < 1e16 {
        // Integer-valued: Python shows "1.0", "42.0", "-3.0"
        return format!("{:.1}", f);
    }
    let abs = f.abs();
    if !(1e-4..1e16).contains(&abs) {
        // Exponent form, Python style: signed exponent with at least two
        // digits ("1e+16", "1e-05", "1e+100"). Rust's `{:e}` emits
        // "1e16" / "1e-5" (unsigned, unpadded) and `{}` never uses
        // exponent form for f64, so always go through `{:e}` here.
        let s = format!("{:e}", f);
        if let Some((mantissa, exp)) = s.split_once('e') {
            let (sign, digits) = match exp.strip_prefix('-') {
                Some(d) => ('-', d),
                None => ('+', exp),
            };
            let padded = if digits.len() < 2 {
                format!("0{digits}")
            } else {
                digits.to_string()
            };
            format!("{mantissa}e{sign}{padded}")
        } else {
            s
        }
    } else {
        format!("{}", f)
    }
}

/// Serialize a slice of [`ToolDef`] as a Python-`json.dumps` list of objects.
///
/// Each tool is serialized as `{"name": ..., "description": ..., "parameters": ...}`
/// with keys in that order (matching the Python `v1_tools` dict insertion order).
pub fn tools_to_json(tools: &[ToolDef]) -> String {
    let values: Vec<Value> = tools
        .iter()
        .map(|t| {
            let mut m = Map::new();
            m.insert("name".into(), Value::String(t.name.clone()));
            m.insert("description".into(), Value::String(t.description.clone()));
            if let Some(params) = &t.parameters {
                m.insert("parameters".into(), params.clone());
            }
            Value::Object(m)
        })
        .collect();
    dump_python_json(&Value::Array(values))
}

/// Build the v1 tool list with runtime-injected enums.
///
/// `apps`, `snippets`, `styles` are the top-N lists injected at runtime.
/// The other enums (transforms, languages, media actions, edit actions)
/// are fixed in the v1 spec.
pub fn v1_tools(apps: &[String], snippets: &[String], styles: &[String]) -> Vec<ToolDef> {
    let str_enum = |vals: &[String]| -> Value {
        Value::Array(vals.iter().map(|s| Value::String(s.clone())).collect())
    };
    let obj_schema = |props: Map<String, Value>, required: &[&str]| -> Value {
        let mut m = Map::new();
        m.insert("type".into(), Value::String("object".into()));
        m.insert("properties".into(), Value::Object(props));
        if !required.is_empty() {
            let req: Vec<Value> = required
                .iter()
                .map(|s| Value::String((*s).to_string()))
                .collect();
            m.insert("required".into(), Value::Array(req));
        }
        Value::Object(m)
    };
    let str_prop = |v: Value| -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("type".into(), Value::String("string".into()));
        if let Value::Array(_) = v {
            m.insert("enum".into(), v);
        }
        m
    };

    let mut tools = Vec::new();

    // open_app
    {
        let mut props = Map::new();
        props.insert("app".into(), Value::Object(str_prop(str_enum(apps))));
        tools.push(ToolDef {
            name: "open_app".into(),
            description: "Open an application by name.".into(),
            parameters: Some(obj_schema(props, &["app"])),
        });
    }
    // web_search
    {
        let mut props = Map::new();
        props.insert(
            "query".into(),
            Value::Object({
                let mut m = Map::new();
                m.insert("type".into(), Value::String("string".into()));
                m
            }),
        );
        tools.push(ToolDef {
            name: "web_search".into(),
            description: "Search the web for a query.".into(),
            parameters: Some(obj_schema(props, &["query"])),
        });
    }
    // transform_selection
    {
        let transforms = ["uppercase", "lowercase", "titlecase", "capitalize"];
        let languages = ["en", "fr", "de", "ja"];
        let mut props = Map::new();
        props.insert(
            "transform".into(),
            Value::Object(str_prop(Value::Array(
                transforms
                    .iter()
                    .map(|s| Value::String((*s).to_string()))
                    .collect(),
            ))),
        );
        props.insert(
            "language".into(),
            Value::Object(str_prop(Value::Array(
                languages
                    .iter()
                    .map(|s| Value::String((*s).to_string()))
                    .collect(),
            ))),
        );
        tools.push(ToolDef {
            name: "transform_selection".into(),
            description: "Transform the current text selection.".into(),
            parameters: Some(obj_schema(props, &["transform"])),
        });
    }
    // insert_autotext
    {
        let mut props = Map::new();
        props.insert(
            "snippet".into(),
            Value::Object(str_prop(str_enum(snippets))),
        );
        tools.push(ToolDef {
            name: "insert_autotext".into(),
            description: "Insert a saved autotext snippet.".into(),
            parameters: Some(obj_schema(props, &["snippet"])),
        });
    }
    // switch_style
    {
        let mut props = Map::new();
        props.insert("style".into(), Value::Object(str_prop(str_enum(styles))));
        tools.push(ToolDef {
            name: "switch_style".into(),
            description: "Switch the current text style.".into(),
            parameters: Some(obj_schema(props, &["style"])),
        });
    }
    // draft_message
    {
        let mut props = Map::new();
        let mut rec = Map::new();
        rec.insert("type".into(), Value::String("string".into()));
        props.insert("recipient".into(), Value::Object(rec));
        let mut body = Map::new();
        body.insert("type".into(), Value::String("string".into()));
        props.insert("body".into(), Value::Object(body));
        tools.push(ToolDef {
            name: "draft_message".into(),
            description: "Draft a message to a recipient.".into(),
            parameters: Some(obj_schema(props, &["recipient", "body"])),
        });
    }
    // media
    {
        let actions = [
            "play_pause",
            "next",
            "previous",
            "volume_up",
            "volume_down",
            "mute",
        ];
        let mut props = Map::new();
        props.insert(
            "action".into(),
            Value::Object(str_prop(Value::Array(
                actions
                    .iter()
                    .map(|s| Value::String((*s).to_string()))
                    .collect(),
            ))),
        );
        tools.push(ToolDef {
            name: "media".into(),
            description: "Control media playback.".into(),
            parameters: Some(obj_schema(props, &["action"])),
        });
    }
    // edit_text
    {
        let actions = [
            "undo_last_dictation",
            "delete_last_sentence",
            "new_line",
            "new_paragraph",
            "select_all",
        ];
        let mut props = Map::new();
        props.insert(
            "action".into(),
            Value::Object(str_prop(Value::Array(
                actions
                    .iter()
                    .map(|s| Value::String((*s).to_string()))
                    .collect(),
            ))),
        );
        tools.push(ToolDef {
            name: "edit_text".into(),
            description: "Edit the surrounding text.".into(),
            parameters: Some(obj_schema(props, &["action"])),
        });
    }
    // paste_last
    tools.push(ToolDef {
        name: "paste_last".into(),
        description: "Paste the last dictated text.".into(),
        parameters: Some(empty_object_schema()),
    });
    // no_action
    tools.push(ToolDef {
        name: "no_action".into(),
        description: "Do nothing.".into(),
        parameters: Some(empty_object_schema()),
    });

    tools
}

/// Python package parity: tools with no parameters still serialize a full
/// object schema (`{"type": "object", "properties": {}}`), not `{}`.
pub fn empty_object_schema() -> Value {
    let mut schema = Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert("properties".into(), Value::Object(Map::new()));
    Value::Object(schema)
}
