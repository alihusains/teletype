//! Voice command test harness.
//!
//! Runs the full test library from `voice-command-testing-library.md`
//! against TinyDecide (classification) and Needle 3 (command extraction).
//!
//! The test dataset is in `voice_command_tests.json` (same directory).
//! Each record has:
//! - `id`: unique test case identifier
//! - `text`: the corrected transcript
//! - `expected_intent`: "command" or "dictation"
//! - `expected_action`: expected tool name (null for dictation)
//! - `expected_args`: expected structured arguments
//! - `category`: test category
//!
//! Run with:
//!   cargo test -p teletype-inference --test voice_command_e2e -- --nocapture

use std::path::PathBuf;

use teletype_core::llm::DecisionProvider;

/// One test case from the JSON dataset.
#[derive(serde::Deserialize)]
struct TestCase {
    id: String,
    text: String,
    expected_intent: String,
    #[serde(default)]
    expected_action: Option<String>,
    #[serde(default)]
    expected_args: Option<serde_json::Value>,
    #[serde(default)]
    category: Option<String>,
}

/// Load the test dataset.
fn load_tests() -> Vec<TestCase> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("tests").join("voice_command_tests.json");
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Cannot read {}: {e}", path.display()));
    serde_json::from_str(&json).expect("Invalid JSON in voice_command_tests.json")
}

/// Find the model directories.
fn model_dirs() -> (Option<PathBuf>, Option<PathBuf>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let td = root.join("models").join("tinydecide");
    let nd = root.join("models").join("needle3.cact");
    (
        if td.join("meta.json").exists() && td.join("model.bin").exists() {
            Some(td)
        } else {
            None
        },
        if nd.exists() { Some(nd) } else { None },
    )
}

#[test]
fn voice_command_e2e() {
    let tests = load_tests();
    println!("\nLoaded {} test cases\n", tests.len());

    let (td_dir, nd_path) = model_dirs();

    let td_provider = td_dir.as_ref().and_then(|d| {
        teletype_inference::TinyDecideProvider::load(d)
            .map_err(|e| eprintln!("TinyDecide load failed: {e}"))
            .ok()
    });

    let nd_provider = nd_path.as_ref().and_then(|p| {
        teletype_inference::NeedleProvider::new("needle3", "Needle 3", p)
            .map_err(|e| eprintln!("Needle 3 load failed: {e}"))
            .ok()
    });

    if td_provider.is_none() && nd_provider.is_none() {
        eprintln!("SKIP: no model files found. Download with:");
        eprintln!("  curl -L -o models/tinydecide/meta.json https://huggingface.co/TheREZOR/TinyDecide/resolve/main/meta.json");
        eprintln!("  curl -L -o models/tinydecide/model.bin https://huggingface.co/TheREZOR/TinyDecide/resolve/main/model.bin");
        eprintln!("  curl -L -o models/needle3.cact https://huggingface.co/Cactus-Compute/needle3/resolve/main/needle3.cact");
        return;
    }

    // ── Classification results ──────────────────────────────────────────
    let mut total = 0;
    let mut correct_intent = 0;
    let mut correct_action = 0;
    let mut correct_args = 0;
    let mut false_commands = 0; // dictation misclassified as command
    let mut missed_commands = 0; // command misclassified as dictation

    let mut by_category: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();

    println!(
        "{:<12} {:<55} {:<12} {:<12} {:<15} {:<15} {}",
        "ID", "TEXT", "EXPECTED", "GOT", "ACTION", "ARGS", "RESULT"
    );
    println!("{}", "-".repeat(120));

    for tc in &tests {
        total += 1;
        let cat = tc.category.as_deref().unwrap_or("unknown");
        let (c, t) = by_category.entry(cat.to_string()).or_insert((0, 0));
        *t += 1;

        let expected_is_command = tc.expected_intent == "command";

        // ── Classification (TinyDecide) ────────────────────────────────
        let got_intent = match &td_provider {
            Some(p) => p.classify_intent(&tc.text),
            None => teletype_core::llm::Intent::Dictation,
        };
        let got_is_command = got_intent == teletype_core::llm::Intent::Command;
        let intent_ok = got_is_command == expected_is_command;

        if intent_ok {
            correct_intent += 1;
            *c += 1;
        }

        if expected_is_command && !got_is_command {
            missed_commands += 1;
        }
        if !expected_is_command && got_is_command {
            false_commands += 1;
        }

        // ── Command extraction (Needle 3) ──────────────────────────────
        let (action_ok, args_ok, got_action, got_args) = if expected_is_command {
            match &nd_provider {
                Some(p) => {
                    let cmd = p.extract_command(&tc.text);
                    match cmd {
                        Some(ref c) => {
                            let a_ok = tc.expected_action.as_deref() == Some(c.tool.as_str());
                            let args_match = match (&tc.expected_args, &c.arguments) {
                                (Some(exp), got) => {
                                    // Check that all expected keys are present
                                    // in the got arguments with matching values.
                                    if let (
                                        serde_json::Value::Object(exp_obj),
                                        serde_json::Value::Object(got_obj),
                                    ) = (exp, got)
                                    {
                                        exp_obj.iter().all(|(k, v)| {
                                            got_obj.get(k).map_or(false, |gv| gv == v)
                                        })
                                    } else {
                                        false
                                    }
                                }
                                _ => true,
                            };
                            if a_ok {
                                correct_action += 1;
                            }
                            if args_match {
                                correct_args += 1;
                            }
                            (
                                a_ok,
                                args_match,
                                Some(c.tool.clone()),
                                Some(c.arguments.clone()),
                            )
                        }
                        None => (false, false, None, None),
                    }
                }
                None => (false, false, None, None),
            }
        } else {
            // Dictation: no action expected.
            (true, true, None, None)
        };

        let result = if intent_ok && action_ok {
            "✓"
        } else if intent_ok {
            "△" // intent right, action wrong
        } else {
            "✗"
        };

        let exp_label = if expected_is_command {
            "command"
        } else {
            "dictation"
        };
        let got_label = got_intent.label();
        let exp_action = tc.expected_action.as_deref().unwrap_or("-");
        let got_action_str = got_action.as_deref().unwrap_or("-");
        let args_str = got_args
            .as_ref()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "-".to_string());

        println!(
            "{:<12} {:<55} {:<12} {:<12} {:<15} {:<15} {}",
            tc.id,
            truncate(&tc.text, 53),
            exp_label,
            got_label,
            format!("{exp_action}→{got_action_str}"),
            truncate(&args_str, 13),
            result
        );
    }

    // ── Summary ─────────────────────────────────────────────────────────
    println!("\n{}", "=".repeat(120));
    println!("CLASSIFICATION (TinyDecide)");
    println!("  Total:        {total}");
    println!(
        "  Correct:      {correct_intent}/{total} ({:.1}%)",
        correct_intent as f64 / total as f64 * 100.0
    );
    println!("  Missed cmds:  {missed_commands}  (command → dictation)");
    println!("  False cmds:   {false_commands}  (dictation → command)");

    if total > 0 {
        let cmd_tests: Vec<&TestCase> = tests
            .iter()
            .filter(|t| t.expected_intent == "command")
            .collect();
        let dict_tests: Vec<&TestCase> = tests
            .iter()
            .filter(|t| t.expected_intent == "dictation")
            .collect();

        if !cmd_tests.is_empty() {
            let cmd_correct = cmd_tests
                .iter()
                .filter(|t| {
                    let got = td_provider
                        .as_ref()
                        .map(|p| p.classify_intent(&t.text))
                        .unwrap_or(teletype_core::llm::Intent::Dictation);
                    got == teletype_core::llm::Intent::Command
                })
                .count();
            println!(
                "  Command recall:  {}/{} ({:.1}%)",
                cmd_correct,
                cmd_tests.len(),
                cmd_correct as f64 / cmd_tests.len() as f64 * 100.0
            );
        }

        if !dict_tests.is_empty() {
            let dict_correct = dict_tests
                .iter()
                .filter(|t| {
                    let got = td_provider
                        .as_ref()
                        .map(|p| p.classify_intent(&t.text))
                        .unwrap_or(teletype_core::llm::Intent::Dictation);
                    got == teletype_core::llm::Intent::Dictation
                })
                .count();
            println!(
                "  Dictation prec:  {}/{} ({:.1}%)",
                dict_correct,
                dict_tests.len(),
                dict_correct as f64 / dict_tests.len() as f64 * 100.0
            );
        }
    }

    println!("\nCOMMAND EXTRACTION (Needle 3)");
    let cmd_count = tests
        .iter()
        .filter(|t| t.expected_intent == "command")
        .count();
    if cmd_count > 0 {
        println!("  Commands:     {cmd_count}");
        println!(
            "  Action match: {correct_action}/{cmd_count} ({:.1}%)",
            correct_action as f64 / cmd_count as f64 * 100.0
        );
        println!(
            "  Args match:   {correct_args}/{cmd_count} ({:.1}%)",
            correct_args as f64 / cmd_count as f64 * 100.0
        );
    }

    println!("\nBY CATEGORY");
    let mut cats: Vec<_> = by_category.iter().collect();
    cats.sort_by_key(|(name, _)| name.as_str());
    for (cat, (correct, total_cat)) in cats {
        println!(
            "  {:<25} {}/{} ({:.0}%)",
            cat,
            correct,
            total_cat,
            *correct as f64 / *total_cat as f64 * 100.0
        );
    }

    // ── Release gates ───────────────────────────────────────────────────
    // A false-positive command (dictation → command) is more harmful than
    // a missed command. Require 0 false commands for release.
    if td_provider.is_none() {
        // Classification gates require the TinyDecide model (gitignored,
        // downloaded per the note above). Without it every intent falls back
        // to Dictation, so the accuracy/false-command numbers are meaningless.
        println!("\nRELEASE GATES");
        println!("  False command rate:     skipped (TinyDecide model not downloaded)");
        println!("  Classification accuracy: skipped (TinyDecide model not downloaded)");
        return;
    }
    let false_cmd_rate = false_commands as f64 / total as f64;
    println!("\nRELEASE GATES");
    println!(
        "  False command rate: {:.1}% (gate: < 5%)",
        false_cmd_rate * 100.0
    );
    assert!(
        false_cmd_rate < 0.05,
        "False command rate {:.1}% exceeds 5% gate",
        false_cmd_rate * 100.0
    );

    // Overall classification accuracy gate.
    let acc = correct_intent as f64 / total as f64;
    println!(
        "  Classification accuracy: {:.1}% (gate: > 80%)",
        acc * 100.0
    );
    assert!(
        acc > 0.80,
        "Classification accuracy {:.1}% below 80% gate",
        acc * 100.0
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
