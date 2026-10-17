//! Unit tests for the command-suite comparison/normalization logic
//! (Addendum A.4, task 7). The pure functions live in the
//! `command_suite` example; `include!` pulls them in so they can be
//! tested here (example files are not compiled into the crate's test
//! targets).

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/command_suite.rs"
));

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scores_known_correct_pair() {
        let expected = json!({"type": "open_app", "tool": "open_app", "args": {"app": "safari"}});
        let got = json!({"name": "open_app", "arguments": {"app": "safari"}});
        let row = score_clip(Some(&expected), Some(&got));
        assert_eq!(row["correct"], true);
    }

    #[test]
    fn scores_known_wrong_pair() {
        let expected = json!({"type": "open_app", "tool": "open_app", "args": {"app": "safari"}});
        let got = json!({"name": "open_app", "arguments": {"app": "chrome"}});
        let row = score_clip(Some(&expected), Some(&got));
        assert_eq!(row["correct"], false);
    }

    #[test]
    fn negative_expected_scores_no_call() {
        let got = json!({"name": "none", "arguments": {}});
        let row = score_clip(None, Some(&got));
        assert_eq!(row["correct"], true);
    }

    #[test]
    fn negative_expected_scores_wrong_call() {
        let got = json!({"name": "open_app", "arguments": {"app": "mail"}});
        let row = score_clip(None, Some(&got));
        assert_eq!(row["correct"], false);
    }

    #[test]
    fn trims_string_args() {
        let a = &norm_args(&json!({"app": "  safari  "}))["app"];
        assert_eq!(a, "safari");
    }

    #[test]
    fn normalizes_needle_function_calls_shape() {
        let v = json!({"function_calls": [{"name": "open_app", "arguments": {"app": "mail"}}]});
        let (name, args) = normalize_decision(&v).unwrap();
        assert_eq!(name, "open_app");
        assert_eq!(args["app"], "mail");
    }

    #[test]
    fn sha256_12_matches_known_vector() {
        // sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(sha256_12(b""), "e3b0c44298fc");
    }
}
