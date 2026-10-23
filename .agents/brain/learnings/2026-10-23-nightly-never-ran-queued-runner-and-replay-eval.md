# L052 — A nightly workflow can be "green-ish" forever by never running

- **ID:** L052
- **Date:** 2026-10-23
- **Agent:** OpenCode (CI/CD repair)
- **Area:** `.github/workflows/nightly.yml`, `fuzz/`, `Cargo.toml` (`ort` pin), `crates/teletype-speech/build.rs`
- **Status:** verified locally (`cargo +nightly fuzz build` exit 0, eval gate PASS, workflow YAML parses); CI run pending

## Problem

The Nightly workflow had never produced a completed run, and the fuzz job could
not build at all. Neither failure announced itself as a configuration error.

## 1. An offline self-hosted label queues, it does not fail

`E2E (self-hosted macOS)` targeted `runs-on: [self-hosted, macos,
macos-self-hosted]` with no runner registered for those labels. GitHub does not
fail such a job: it holds it in `queued` until the 24-hour limit, which leaves
the entire run `queued` (run `39744047918` sat that way for ~21 h). Every
scheduled run therefore looked "in progress" instead of broken.

Fix: job-level `if: vars.TELETYPE_SELF_HOSTED_E2E == 'true'`. The job now
reports `skipped`, the run completes, and the self-hosted job returns when
someone registers the runner and sets the repository variable (`gh variable set
TELETYPE_SELF_HOSTED_E2E --body true`).

Rule: any job that targets a self-hosted label must be behind an explicit
enable switch, because the failure mode is silence, not red.

## 2. `--asr none` is replay mode and requires hypotheses

The eval step ran:

```bash
cargo run -p teletype-eval --bin eval -- --corpus evals/corpus --thresholds ...
```

and exited 1 on every nightly with:

```
error: no hypotheses file: --engine none requires --hypotheses <file> (replay mode)
```

Fix: pass `--asr none --hypotheses evals/hypotheses.jsonl` and say "replay" in
the job name. A native-ASR nightly still needs a model staging step; the old
comment claimed native ASR while the invocation could never have done it.

## 3. A detached fuzz workspace resolves its own versions

`fuzz/` is its own workspace with its own lockfile. A caret requirement
(`ort = "2.0.0-rc.13"`) let that lockfile resolve `rc.15`, whose
`SessionBuilder::new()` takes `&Environment`; `teletype-core` then failed to
compile inside the fuzz build while the main workspace was fine.

Fix: pin `ort = "=2.0.0-rc.13"` in the workspace root, with the reason in a
comment, and regenerate `fuzz/Cargo.lock`. Rule: any dependency a second
workspace also resolves must be pinned with `=` when its prerelease series has
breaking patch-level releases.

## 4. Cargo's build-directory layout is not stable across channels

`crates/teletype-speech/build.rs` located the `parakeet-sys` CMake output by
matching `build/parakeet-sys-<hash>/out`. Stable 1.90 writes that; nightly 1.101
writes `build/parakeet-sys/<hash>/out`. The nightly fuzz link therefore failed
with "could not find native static library `parakeet`".

Fix: find the `build` ancestor of `OUT_DIR` and scan both shapes
(`parakeet_build_dirs`). The build script is the only place that knows this
layout, so it is the right place to absorb the difference.

## Verification

- `cargo +nightly fuzz build` → exit 0, links with the nightly layout.
- Replay eval command from the workflow → `threshold gate: PASS`.
- `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/nightly.yml'))"`
  → parses; jobs: `eval-full`, `fuzz`, `concurrency-stress`, `chaos`, `e2e`,
  `offline`.
- Stable `cargo check -p teletype-speech` and clippy both exit 0, so the dual
  layout scan did not break the normal build.

## Rule

- A scheduled workflow that has never completed is not evidence that it passed.
  Check `gh run list --workflow nightly.yml` for `queued`/`skipped` history, not
  just the latest conclusion.
- Gate every self-hosted job behind a repo variable; never let an offline label
  hold a workflow open.
- When a job's comment describes a stronger test than its command performs,
  treat the comment as the bug and rewrite both.
- Pin prerelease dependencies with `=` when a detached workspace (fuzz, tools,
  examples) resolves them independently.
