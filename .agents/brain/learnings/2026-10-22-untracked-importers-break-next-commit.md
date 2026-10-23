# Untracked files that tracked code already imports break the next commit, not HEAD

**ID:** L041
**Date:** 2026-10-22
**Agent:** Qoder (lead session, T7/T9 review)
**Area:** release integrity / git / build
**Status:** verified

## Problem

A working tree can compile perfectly while the commit it is about to become
cannot. The gap is a file that tracked code imports but that git has never been
told about: it is `??` in `git status`, so any commit flow that stages modified
files (`git commit -a`, an IDE "commit all changes") produces a tree that does
not build.

## Symptoms

Nothing local. `cargo check` and `tsc --noEmit` both pass, because the file is
present on disk. The failure appears only on a fresh clone, or in CI, or for
another person: `can't find module sigterm` / `Failed to resolve import
"./lib/dialogs"`.

## Investigation

1. `git status --porcelain | awk '$1=="??"'` - 26 untracked paths in this tree.
2. Keep the ones with source extensions (`.rs .ts .tsx .py .sh .jsonl .json .m`)
   and drop the obvious scratch (logs, `.agents/`, `tasks/`, `evals/runs/`).
3. For each survivor, `git grep -l <basename>` over tracked files only. A hit
   means something that *is* committed already depends on it.
4. Cross-check against the committed state, not the working copy:
   `git show HEAD:<importer> | grep <basename>`. This distinguishes "HEAD is
   already broken" from "the next commit will be".

## Evidence

Found 22 Oct 2026, all prospective (HEAD clean, verified with `git show HEAD:`):

| Untracked file | Committed importer |
|---|---|
| `crates/teletype-desktop/src/sigterm.rs` | `crates/teletype-desktop/src/lib.rs:65` (`mod sigterm;`), `:1029` (`sigterm::install`) |
| `ui/src/lib/dialogs.ts` | `ui/src/App.tsx`, `ui/src/screens/AutoTextScreen.tsx`, `ui/src/screens/DictionaryScreen.tsx` |
| `scripts/eval/polish_behavior_gate.py`, `evals/polish-adversarial/behavior-cases.jsonl`, `evals/polish-adversarial/correct-outputs.json` | `.github/workflows/ci.yml` `polish-quality-gate` job (`--list-corpus`, `--self-test`) |

Both build-breaking pairs came from tasks finished the same day: the SIGTERM
handler (T7) and the dialog helper. Neither had been staged.

## Root cause

New files created by an agent are invisible to a modified-files-only commit.
Reviewers check that the tree compiles, which is exactly the test that cannot
see this.

## Fix

Staged the five files (`git add <paths>`, no commit). The general fix is to run
the untracked-importer check as the last step before any commit, and to prefer
`git add <specific paths>` over `-a`.

## Verification

`git status --short` shows all five as `A `. `cargo check -p teletype-desktop`
exit 0. The CI step's inputs now exist in the index, so a fresh clone can run
`polish-quality-gate`.

## Reusable lesson

"Green build locally" is not evidence that "the committed tree builds". The
dependency that matters is the one git does not know about. Also: a CI job that
invokes an untracked script is a guaranteed red on the next push, and it looks
like a CI bug rather than a missing `git add`.
