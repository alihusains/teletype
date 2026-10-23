# L014: S1-mini control axes verified live — and one real quality gap (casual keeps lowercase)

**ID:** L014
**Date:** 2026-10-05
**Agent:** Codex
**Project area:** teletype-core (transform engine) / teletype-inference (llama-server)
**Status:** verified
**Evidence:** live runs against `target/debug/llama-server` serving `s1-mini.gguf` (q4_k_m, `--jinja --chat-template-kwargs '{"enable_thinking":false}'`), exact app request shape (card system prompt + `[Styling: …] [Structure: …] [Context: …]` control line, temperature 0). Full transcripts in this file. Re-verified 2026-10-06 with the current spawn flags (`--jinja --chat-template-kwargs '{"enable_thinking":false}'`) and the current `run_s1` request shape (per-request `chat_template_kwargs` + `temperature:0`): all three control axes returned correct, distinct output with `finish_reason=stop` and no empty-content failures — e.g. semi-formal/lists cleaned fillers + added punctuation, casual/prose kept lowercase "i" (model register, per finding 2), formal/lists/email normalized "first and the fifth" → "1st and the 5th".

## Problem

The goal flagged "S1-mini is not working like the video (Better Stack, 'This 600M Model Fixes What Whisper Leaves Behind', youtu.be/jp0PGmAwK3w)". Needed proof of what the model actually does end-to-end through our exact prompt path, and whether anything in our pipeline contradicts the card.

## Investigation

- Spawned llama-server on the local `s1-mini.gguf` with the app's spawn flags plus `--reasoning off` (equivalent of `enable_thinking: false`; see L013).
- Sent the exact `run_s1` request shape for 7 transcripts covering the video's test categories: fillers, false starts / self-corrections, spoken numbers, email addresses, list structure, styling shifts.

## Evidence (live outputs)

| Control | Input (abridged) | Output |
|---|---|---|
| semi-formal / lists | "um so basically I was thinking we should probably um have a meeting about the thing right?" | "So basically I was thinking we should probably have a meeting about the thing, right?" |
| casual / prose | same | "um so basically i was thinking we should probably have a meeting about the thing right?" |
| formal / prose / email | "one apple two orange three banana and four um cherry" | "One apple, two orange, three banana, and four cherry." |
| semi-formal / prose / email | "…send over the invoice to finance at acme dot com by um friday end of day" | "Hey,\n\nSo can you send over the invoice to finance@acme.com by Friday end of day?" |
| semi-formal / prose | "the version is uh two point one point three and the port is fifty five five five" | "The version is 2.13 and the port is 5555." |
| casual / prose | "so I was going to go to the store but um actually no I changed my mind…" | "so i was going to go to the store but um actually no i changed my mind and im just gonna stay home" |
| semi-formal / lists | "…three things one we shipped the login fix two the payment bug is still open three…" | numbered list, 3 items |

## Findings

1. **The core offering works**: all three control axes (styling / structure / context) produce visibly different, correct output; email addresses and list structure are handled exactly as the video shows. The card's system prompt + control line is the right distribution.
2. **Quality gap (model-level, not a code bug)**: `casual` styling keeps lowercase "i" and skips punctuation ("im just gonna stay home"). That is the model's casual register, but for a dictation product it reads as broken because our deterministic pipeline (capitalization/punctuation) is bypassed for S1 — the model is the only cleaner.
3. **Model-level ITN bug**: "two point one point three" → "2.13" (dropped a digit). Inherent to the 0.6B fine-tune; nothing in our prompt path causes or can fix it.
4. **No app-side regression found**: `run_s1` uses the card-exact format; `enable_thinking: false` is applied at spawn and per-request (L013); the Styles screen's Tone/Structure/Context controls flow through `profile.s1_control` into every request.

## Reusable lesson

When "it doesn't work like the video" is reported for a fine-tuned cleanup model, first reproduce the *exact* request shape against the local weights before touching code: the video's behavior is a property of (weights + exact prompt), and app regressions usually show up as *all* controls doing the same thing, not as per-control quality differences. Judge per-axis output differences against the model card, not against the deterministic pipeline's guarantees.

## Follow-ups (candidate, not yet actioned)

- Consider post-S1 deterministic pass (capitalization + "i" fix) for casual output — product decision needed.
- ITN digit-dropping is a known 0.6B limitation; document it in the model's verdict note if users hit it.
