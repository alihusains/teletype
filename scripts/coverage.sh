#!/usr/bin/env bash
set -euo pipefail

# Generate code coverage using cargo-llvm-cov
# Install: cargo install cargo-llvm-cov
# Needs llvm-tools: rustup component add llvm-tools-preview

HTML_DIR="${COVERAGE_HTML_DIR:-target/llvm-cov/html}"
LCOV_OUT="${COVERAGE_LCOV:-target/llvm-cov/lcov.info}"
JSON_OUT="${COVERAGE_JSON:-target/llvm-cov/coverage.json}"

mkdir -p "$(dirname "$LCOV_OUT")" "$(dirname "$JSON_OUT")" "$HTML_DIR"

cargo llvm-cov --workspace --all-features --all-targets \
  --lcov --output-path "$LCOV_OUT" \
  --json --output-path "$JSON_OUT" \
  --html --output-dir "$HTML_DIR"

echo "HTML:  $HTML_DIR/index.html"
echo "LCOV:  $LCOV_OUT"
echo "JSON:  $JSON_OUT"