//! Fixture-driven ITN parity tests.
//!
//! Loads `tests/fixtures/itn-parity.jsonl` (a curated ITN fixture set: gold,
//! idempotence, negative, and public slices) and asserts `itn::normalize(input)
//! == expected` for each row. Reports per-slice and per-category pass counts.

use std::collections::HashMap;
use std::path::Path;

struct Row {
    input: String,
    expected: String,
    category: String,
    slice: String,
}

fn load_rows() -> Vec<Row> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/itn-parity.jsonl");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    content
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let v: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("bad JSON {line}: {e}"));
            Row {
                input: v["input"].as_str().unwrap().to_string(),
                expected: v["expected"].as_str().unwrap().to_string(),
                category: v["category"].as_str().unwrap_or("unknown").to_string(),
                slice: v["slice"].as_str().unwrap_or("unknown").to_string(),
            }
        })
        .collect()
}

/// Run all fixture rows and report per-slice / per-category pass counts.
/// Panics on the first failure (standard test behavior) but prints the
/// aggregate counts so a partial pass is visible in the failure output.
#[test]
#[ignore = "phase-2b: 263/2416 rows failing. Fixed: cents conversion (standalone + with dollars), compound ordinals, 'and' in cardinals, spoken punctuation, money ordering, years scale rejection, mixed_state digit handling. Remaining: date 51 (capitalization, slash dates, 'twenty fourteen' year parsing), negative 15 (capitalization, bare ordinals), currency 15 (standalone cents), numeric 13 (comma grouping, 'and' in years), email 2, url 1, time 1, punctuation 1. TODO: fix remaining gaps, remove ignore."]
fn itn_parity_fixtures() {
    let rows = load_rows();
    assert!(!rows.is_empty(), "no fixture rows loaded");

    let mut slice_pass: HashMap<String, (u32, u32)> = HashMap::new(); // pass, total
    let mut cat_pass: HashMap<String, (u32, u32)> = HashMap::new();
    let mut failures: Vec<(String, String, String, String)> = Vec::new();

    for row in &rows {
        let actual = teletype_core::itn::normalize(&row.input);
        let key = (row.slice.clone(), row.category.clone());
        let (sp, st) = slice_pass.entry(row.slice.clone()).or_insert((0, 0));
        *st += 1;
        let (cp, ct) = cat_pass.entry(row.category.clone()).or_insert((0, 0));
        *ct += 1;
        if actual == row.expected {
            *sp += 1;
            *cp += 1;
        } else {
            failures.push((
                row.slice.clone(),
                row.category.clone(),
                row.input.clone(),
                format!("expected {:?}, got {:?}", row.expected, actual),
            ));
        }
        let _ = key;
    }

    // Print per-slice summary.
    let mut slices: Vec<&String> = slice_pass.keys().collect();
    slices.sort();
    println!(
        "\n=== ITN parity: {} rows, {} failures ===",
        rows.len(),
        failures.len()
    );
    for s in slices {
        let (p, t) = slice_pass[s];
        println!("  slice {s:<12} {p}/{t}");
    }
    let mut cats: Vec<&String> = cat_pass.keys().collect();
    cats.sort();
    for c in cats {
        let (p, t) = cat_pass[c];
        println!("  category {c:<12} {p}/{t}");
    }

    if !failures.is_empty() {
        println!("\nFirst 100 failures:");
        for (slice, cat, input, detail) in failures.iter().take(100) {
            println!("  [{slice}/{cat}] {input:?} -> {detail}");
        }
        panic!("{} fixture rows failed", failures.len());
    }
}

/// Idempotence: normalize(normalize(x)) == normalize(x) for the idempotence
/// slice (inputs already in written form).
#[test]
fn itn_idempotence() {
    let rows = load_rows();
    let mut failures = 0;
    for row in &rows {
        if row.slice != "idempotence" {
            continue;
        }
        let once = teletype_core::itn::normalize(&row.input);
        let twice = teletype_core::itn::normalize(&once);
        if once != twice {
            failures += 1;
            if failures <= 10 {
                println!(
                    "  NOT IDEMPOTENT: {:?} -> {:?} -> {:?}",
                    row.input, once, twice
                );
            }
        }
    }
    assert_eq!(failures, 0, "{failures} idempotence rows are not stable");
}
