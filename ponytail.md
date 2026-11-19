# Teletype — optimization & improvement audit

**Date:** 2026-11-19 · **Baseline:** ~150K lines Rust + ~39K lines TS, 10 crates, v0.2.19.
Workspace tests green. The code is not rotting — only 2 TODOs, no dead
`todo!`/`unimplemented!` in production paths. The big problems are *shape*, not *bugs*.

## The three things that matter most

**1. `dictation.rs` is a 15,797-line god module (380 fns, 233 `.clone()`s) — the #1 technical risk**
The whole dictation controller, streaming takes, VAD collection, hotkeys, pill
throttling, language chips, and session state all live in one file. This is where
most bugs have historically come from (the SIGTERM/`AppHandle` saga, stuck-pill,
the L063 screen-mounting storm). It's not one clean split; it's already half-split
(`dictation_stages.rs`, `command_mode.rs`, `committer.rs` exist as siblings). The
lazy next step: extract the `StreamingTake`/VAD/audio side and the `Session`/
state-machine side into their own modules. Don't refactor for the joy of it — do
it when the next dictation bug lands, because the extract *is* the fix.

**2. The human ASR corpus is 0/120 — your primary quality metric is null**
Per the brain's own rules, *no model recommendation may change* until a
human-corpus run exists. Everything about "which ASR is best" is currently
measured on synthesized audio only. This is the single biggest lever on product
quality you can pull, and it's a data-collection task, not a code task. Record
the 120 clips and the whole eval register becomes real.

**3. The honest product gaps (from `usp.md`):**
- **Streaming preview** — you win on ASR latency (142ms p50) but Wispr wins
  perceived end-to-end speed because they stream. The spike findings say
  re-decoding rewrites the whole hypothesis. This is your most-visible
  competitive gap.
- **Polish quality** is *claimed* but not *measured* — the brain explicitly
  labels it a gap.
- **Windows is a stub** — `windows_impl.rs` is 143 lines; hotkey/injection/
  tray/mic not implemented. Either commit to it or stop implying
  "cross-platform."

## Ranked cuts / simplifications

| Tag | Cut | Replacement | Where |
|---|---|---|---|
| **shrink** | `dictation.rs` 15.8K lines | Split `StreamingTake`/VAD + `Session` into 2 modules | `crates/teletype-desktop/src/` |
| **yagni** | `LlamaProvider` stub ("runtime not linked") | Delete; `ServerProvider` is the real path | `teletype-inference` |
| **yagni** | `Whistle` enum variant, `#[allow(dead_code)]` "once catalog entry lands" | Delete until the entry actually lands | `dictation.rs:467` |
| **shrink** | All 13 screens mounted with `display:none`, 12 run polling loops at boot (L063) | Propagate the `active` gate (SettingsScreen precedent) to all screens | `ui/src/App.tsx` |
| **yagni** | Legacy `scratchpad.json` key + migration | Notetaker replaced Scratchpad (D044) — drop the legacy path once migration proven | `persist.rs` |
| **delete** | Root `checkpoint.md`, `agvity.md`, `Super-wispr-DESIGN.md`, etc. — 8+ stale top-level planning docs that predate the brain | Archive to `docs/archives/` (brain is the source of truth) | repo root |

**Net: no deps to remove, but ~2–3K lines of dead weight and a 15K-line file
that's the whole risk surface.**

## Suggested order of attack

1. **Record the 120-clip human corpus** (unblocks the entire eval register —
   highest product leverage, zero code risk).
2. **Split `dictation.rs`** on the next dictation bug, not before.
3. **Propagate the `active` gate** to all 13 screens (small, measured boot-IPC
   win, L063 already scoped it).
4. **Decide Windows: real plan or drop the claim.** Q004 has been open since
   before v0.2.7.
5. **Streaming preview** — the visible Wispr gap; the spike already found the
   re-decode problem, so it's a known-hard problem, budget it as such.
