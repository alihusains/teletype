# T0 — Guard word-diff correctness

Status: verified in main tree.

Implemented the corrected guard policy in `crates/teletype-core/src/transforms/guard.rs`:
- reject on any protected-token removal (no reinsertion);
- restore a substituted protected token only when the unmatched polished token sits exactly at `matched_before == span.start`;
- case-insensitive `bare_lower` LCS so a case-only edit does not displace the following verb.

Updated `crates/teletype-core/tests/polish_punctuation.rs`: live path returns raw on guard reject; final assertion expects Thursday/Friday/Monday and the raw fallback phrase.

Verification: 23 guard tests pass; full `cargo test -p teletype-core` green (1368 lib + all integration suites in the final combined run). No brain change needed.
