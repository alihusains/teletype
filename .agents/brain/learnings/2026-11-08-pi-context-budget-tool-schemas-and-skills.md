---
id: 2026-11-08-pi-context-budget-tool-schemas-and-skills
title: A bare pi turn cost 93k input tokens; most of it was unused tool schemas, and deferring them cut 41k
status: verified
type: debugging
area: tooling / agent runtime / context budget
agent: opencode
tool: opencode/deepseek-v4.1-flash-free
date: 2026-11-08
---

# A bare pi turn cost 93k input tokens; most of it was unused tool schemas, and deferring them cut 41k

## Problem

`pi -p "hey"` in this repo took ~13s and sent 93,166 input tokens. A single-word
prompt should not cost a hundred thousand tokens of prefill. The cost was not the
conversation - it was the static payload shipped on every request.

## Symptoms

- 93,166 `usage.input` on a bare turn, ~13.0s wall.
- 133 active tools, 143,320 chars of tool schemas (~35.8k tokens) - more than the
  entire skill index.
- Skills index alone: ~41k tokens across 288 entries.
- `cacheRead: 0` on every response, which looked like a broken prefix cache.

## Initial hypotheses

1. The provider prefix cache is broken, so every turn re-prefills from scratch.
2. Skills are the bulk of the payload.
3. MCP server declarations are the bulk.

## Investigation steps and evidence

- Split the payload: tool schemas 35.8k tokens, skills index ~41k, base system
  prompt the remainder. MCP servers contributed only ~1.9k.
- Direct provider probes showed prefix caching *does* work: a 120k prefix went
  53.8s cold -> 6.4s warm. `cacheRead: 0` is meaningless here because the
  endpoint omits cached-token usage entirely - do not treat it as an absence of
  caching.
- pi-lens reported `cache_context` divergences at message index 1-2. These were
  an artifact of truncated sequence hashing
  (`unknownEvidenceReasons: sequence-hash-truncated: 1483/1492`), with
  `prefix-broke: 0`. Not real prefix breaks.
- `cacheWarming: "streaming"` is inert on this model: no `promptCache` TTL and
  zero cost, so it neither helps nor costs.
- MCP laziness already existed through `pi-mcp-adapter`; that config had
  `"exposure": "direct"`, an explicit opt-out, not a missing feature.
- Endpoint latency variance is large: the identical 25k prompt returned 4.7s and
  then 17.0s. Single-sample latency comparisons are worthless; compare tokens.

## Root cause

The prompt carried every installed tool's full schema whether or not the model
could need it. Two independent levers, both reversible:

1. Disable genuinely unused tools (11 pi-lens analysis tools, plus two unused
   package extension sets). Active tools 133 -> 43, schemas 143,320 -> 59,838
   chars (~20.9k tokens).
2. Defer the rest behind a search tool with `pi-deferred-context-engine`, pinning
   only high-frequency tools.

## Fix

- `~/.pi-lens/config.json`: `tools.<name>.enabled: false` for the 11 unused
  analysis tools; keep `lens_diagnostics` and `pi_lens_activate_tools`.
- `~/.pi/agent/settings.json`: unused packages switched to object form with
  `"extensions": []` (skills left intact).
- `~/.pi/agent/mcp.json`: `"exposure": "codemode"` for servers not needed as
  direct tools.
- `~/.pi/agent/deferred-tools.json`: `deferByDefault: true`,
  `deferSkills: false`, `promotionLifetime: "session"`, and 11 pinned tools
  (`read`, `bash`, `edit`, `write`, `glob`, `ffgrep`, `fffind`, `mcp`,
  `subagent`, `ask_user_question`, `web_search`).

## Verification

Controlled A/B, same directory, same `pi -p "hey"`, only the DCE config differing:

| Config | Input tokens | Delta |
|---|---|---|
| DCE off | 66,492 | - |
| DCE on, 4 pins | 47,400 | -19,092 (-28.7%) |
| DCE on, 11 pins | 52,086 | -14,406 (-21.7%) |
| DCE on, 12 pins (+`todo`) | 53,785 | -12,707 (-19.1%) |

`@juicesharp/rpiv-todo` costs +465 tokens while deferred (the extension's
system-prompt guidance) and +1,699 with its `todo` tool pinned. Pin it: a
deferred tool whose guidance still says "track this with todos" is a dangling
instruction, and the first call needs a search round-trip.

Against the original 93,166 baseline, 52,086 is -41,080 tokens (-44%).
Token counts were stable across repeats (47,400 and 47,400; 66,492 and 66,492).
Wall clock was not: the same DCE-on prompt took 7.3s and 25.3s on consecutive
runs, and DCE-off took 24.3s and 7.5s - the noise swamps any latency claim.

Skills were verified untouched: 288 entries in the system prompt with DCE on
and off (`deferSkills: false`), because the user explicitly refused to curate
the skill index. `systemPrompt` shrank only 6,617 chars, from removed tool
snippets.

## Reusable lesson

Measure the static payload before blaming the model or the network. On a fat
agent install the tool schemas can rival the skill index, and a deferring layer
that hides *declarations* (not tool names) will not show up in
`getActiveToolNames()` - only in the request's `usage.input`. Pin anything used
most turns: each deferred tool costs one full-prompt round-trip to promote
(~47k tokens here), so a 600-token schema is cheap by comparison. Also: an
endpoint that omits cached-token usage reports `cacheRead: 0` while caching
perfectly - probe the provider directly before "fixing" the cache.
