//! Byte-identical compat test: Rust `tools_to_json` vs Python `json.dumps`.
//!
//! The fixtures in `tests/fixtures/tools_json/NN.expected` were generated
//! by `scripts/gen_decider_fixtures.py` using Python's stdlib
//! `json.dumps(tools, ensure_ascii=True)`. This test rebuilds the same
//! 20 tool lists in Rust and asserts byte-identical output.

use teletype_decider::tool_json::{dump_python_json, tools_to_json, v1_tools, ToolDef};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tools_json");

const APPS: &[&str] = &["terminal", "safari", "notes", "mail", "finder", "code"];
const SNIPPETS: &[&str] = &["thanks", "regards", "br", "signature"];
const STYLES: &[&str] = &["body", "heading", "code"];

fn to_strings(vals: &[&str]) -> Vec<String> {
    vals.iter().map(|s| s.to_string()).collect()
}

fn expected_path(n: usize) -> std::path::PathBuf {
    std::path::Path::new(FIXTURE_DIR).join(format!("{n:02}.expected"))
}

fn read_expected(n: usize) -> String {
    let path = expected_path(n);
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("missing fixture {path:?}: {e}"));
    // Fixtures are written without trailing newline
    content
}

fn build_lists() -> Vec<Vec<ToolDef>> {
    let full = v1_tools(
        &to_strings(APPS),
        &to_strings(SNIPPETS),
        &to_strings(STYLES),
    );
    let mut lists: Vec<Vec<ToolDef>> = Vec::new();
    for i in 0..10 {
        // Saturating slices, mirroring the Python reference
        // (`APPS[: i + 1]`, `SNIPPETS[: max(1, i)]`, `STYLES[: max(1, i)]`
        // never overflow in Python, they just return fewer elements).
        lists.push(v1_tools(
            &to_strings(&APPS[..(i + 1).min(APPS.len())]),
            &to_strings(&SNIPPETS[..i.max(1).min(SNIPPETS.len())]),
            &to_strings(&STYLES[..i.max(1).min(STYLES.len())]),
        ));
    }
    // 11: paste_last + no_action only
    lists.push(vec![full[8].clone(), full[9].clone()]);
    // 12: empty tool list
    lists.push(vec![]);
    // 13: unicode names and descriptions
    lists.push(vec![
        ToolDef {
            name: "ouvrir_terminal".into(),
            description: "Ouvre le terminal. 日本語のツールです。".into(),
            parameters: Some(serde_json::Value::Object(serde_json::Map::new())),
        },
        ToolDef {
            name: "càffè".into(),
            description: "☕ — \"quotes\" and \\ backslashes".into(),
            parameters: Some(serde_json::Value::Object(serde_json::Map::new())),
        },
    ]);
    // 14: escapes in descriptions and enum values
    {
        let mut props = serde_json::Map::new();
        let mut choice = serde_json::Map::new();
        choice.insert("type".into(), "string".into());
        choice.insert(
            "enum".into(),
            serde_json::json!(["a\"b", "c\\d", "e\nf", "g\th"]),
        );
        props.insert("choice".into(), serde_json::Value::Object(choice));
        let mut params = serde_json::Map::new();
        params.insert("type".into(), "object".into());
        params.insert("properties".into(), serde_json::Value::Object(props));
        lists.push(vec![ToolDef {
            name: "escaped".into(),
            description: "line1\nline2\ttabbed \"quoted\" \\ backslash \u{08}\u{0C}\r ctrl".into(),
            parameters: Some(serde_json::Value::Object(params)),
        }]);
    }
    // 15: long description
    {
        let desc = "This tool does a lot of things. First it does one thing, then it does another thing, and finally it does a third thing that is closely related to the first two. It is described in detail because the NeuDecide model encodes the serialized tool list into its context, so longer descriptions cost tokens but may improve disambiguation between similar tools. Second paragraph: the description may span multiple sentences and contain punctuation such as (parentheses), [brackets], {braces}, and a slash in a path like /usr/local/bin/tool. Third paragraph: unicode accents éàü and symbols ©®™ are escaped as \\u sequences in the serialized form.";
        let mut props = serde_json::Map::new();
        let mut option = serde_json::Map::new();
        option.insert("type".into(), "string".into());
        option.insert("enum".into(), serde_json::json!(["one", "two", "three"]));
        props.insert("option".into(), serde_json::Value::Object(option));
        let mut params = serde_json::Map::new();
        params.insert("type".into(), "object".into());
        params.insert("properties".into(), serde_json::Value::Object(props));
        lists.push(vec![ToolDef {
            name: "verbose_tool".into(),
            description: desc.into(),
            parameters: Some(serde_json::Value::Object(params)),
        }]);
    }
    // 16: nested parameter structure
    {
        let mut kind = serde_json::Map::new();
        kind.insert("type".into(), "string".into());
        kind.insert("enum".into(), serde_json::json!(["file", "folder"]));
        let mut path_p = serde_json::Map::new();
        path_p.insert("type".into(), "string".into());
        let mut target_props = serde_json::Map::new();
        target_props.insert("kind".into(), serde_json::Value::Object(kind));
        target_props.insert("path".into(), serde_json::Value::Object(path_p));
        let mut target = serde_json::Map::new();
        target.insert("type".into(), "string".into());
        target.insert("type".into(), "object".into());
        target.insert("properties".into(), serde_json::Value::Object(target_props));
        let mut items = serde_json::Map::new();
        items.insert("type".into(), "string".into());
        let mut flags = serde_json::Map::new();
        flags.insert("type".into(), "array".into());
        flags.insert("items".into(), serde_json::Value::Object(items));
        let mut count = serde_json::Map::new();
        count.insert("type".into(), "integer".into());
        count.insert("minimum".into(), serde_json::json!(0));
        let mut ratio = serde_json::Map::new();
        ratio.insert("type".into(), "number".into());
        let mut props = serde_json::Map::new();
        props.insert("target".into(), serde_json::Value::Object(target));
        props.insert("flags".into(), serde_json::Value::Object(flags));
        props.insert("count".into(), serde_json::Value::Object(count));
        props.insert("ratio".into(), serde_json::Value::Object(ratio));
        let mut params = serde_json::Map::new();
        params.insert("type".into(), "object".into());
        params.insert("properties".into(), serde_json::Value::Object(props));
        params.insert("required".into(), serde_json::json!(["target"]));
        lists.push(vec![ToolDef {
            name: "nested".into(),
            description: "Tool with nested parameter structure.".into(),
            parameters: Some(serde_json::Value::Object(params)),
        }]);
    }
    // 17: no parameters key (empty object)
    lists.push(vec![ToolDef {
        name: "bare".into(),
        description: "No parameters key.".into(),
        parameters: None,
    }]);
    // 18: flat parameter mapping
    {
        let mut when = serde_json::Map::new();
        when.insert("type".into(), "string".into());
        let mut who = serde_json::Map::new();
        who.insert("type".into(), "string".into());
        let mut params = serde_json::Map::new();
        params.insert("when".into(), serde_json::Value::Object(when));
        params.insert("who".into(), serde_json::Value::Object(who));
        lists.push(vec![ToolDef {
            name: "flat".into(),
            description: "Flat parameter mapping, not a JSON-Schema object.".into(),
            parameters: Some(serde_json::Value::Object(params)),
        }]);
    }
    // 19: duplicate names
    lists.push(vec![
        ToolDef {
            name: "open_app".into(),
            description: "Open an application.".into(),
            parameters: Some(serde_json::Value::Object(serde_json::Map::new())),
        },
        ToolDef {
            name: "open_app".into(),
            description: "Open an application (alias).".into(),
            parameters: Some(serde_json::Value::Object(serde_json::Map::new())),
        },
    ]);
    // 20: full v1 set
    lists.push(v1_tools(
        &to_strings(APPS),
        &to_strings(SNIPPETS),
        &to_strings(STYLES),
    ));
    assert_eq!(lists.len(), 20);
    lists
}

#[test]
fn byte_identical_to_python_json_dumps() {
    let lists = build_lists();
    for (i, tools) in lists.iter().enumerate() {
        let n = i + 1;
        let expected = read_expected(n);
        let actual = tools_to_json(tools);
        assert_eq!(
            actual, expected,
            "fixture {n:02}: byte mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
        );
    }
}

#[test]
fn empty_list_serializes_to_empty_array() {
    assert_eq!(tools_to_json(&[]), "[]");
}

#[test]
fn dump_python_json_basic_types() {
    assert_eq!(dump_python_json(&serde_json::json!(null)), "null");
    assert_eq!(dump_python_json(&serde_json::json!(true)), "true");
    assert_eq!(dump_python_json(&serde_json::json!(false)), "false");
    assert_eq!(dump_python_json(&serde_json::json!(42)), "42");
    assert_eq!(dump_python_json(&serde_json::json!(-7)), "-7");
    assert_eq!(dump_python_json(&serde_json::json!("hello")), "\"hello\"");
    assert_eq!(dump_python_json(&serde_json::json!([1, 2, 3])), "[1, 2, 3]");
    assert_eq!(
        dump_python_json(&serde_json::json!({"a": 1, "b": 2})),
        "{\"a\": 1, \"b\": 2}"
    );
}

#[test]
fn dump_python_json_ensure_ascii() {
    // Non-ASCII → \uXXXX
    assert_eq!(dump_python_json(&serde_json::json!("é")), "\"\\u00e9\"");
    // CJK
    assert_eq!(dump_python_json(&serde_json::json!("日")), "\"\\u65e5\"");
    // Astral → surrogate pair
    assert_eq!(dump_python_json(&serde_json::json!("☕")), "\"\\u2615\"");
    assert_eq!(
        dump_python_json(&serde_json::json!("😀")),
        "\"\\ud83d\\ude00\""
    );
}

#[test]
fn dump_python_json_escapes() {
    assert_eq!(dump_python_json(&serde_json::json!("a\"b")), "\"a\\\"b\"");
    assert_eq!(dump_python_json(&serde_json::json!("a\\b")), "\"a\\\\b\"");
    assert_eq!(dump_python_json(&serde_json::json!("a\nb")), "\"a\\nb\"");
    assert_eq!(dump_python_json(&serde_json::json!("a\tb")), "\"a\\tb\"");
    // Slash is NOT escaped (Python does not escape /)
    assert_eq!(
        dump_python_json(&serde_json::json!("/usr/local")),
        "\"/usr/local\""
    );
}

#[test]
fn format_python_float_basic() {
    assert_eq!(teletype_decider::format_python_float(1.0), "1.0");
    assert_eq!(teletype_decider::format_python_float(42.0), "42.0");
    assert_eq!(teletype_decider::format_python_float(-3.0), "-3.0");
    assert_eq!(teletype_decider::format_python_float(0.5), "0.5");
    assert_eq!(teletype_decider::format_python_float(1e16), "1e+16");
    assert_eq!(teletype_decider::format_python_float(1e-5), "1e-05");
}
