# Head-to-head benchmark (Teletype vs Wispr Flow)

`h2h.py` measures "hotkey released → first character on screen" latency,
WER, and failure rate for both apps on the same recorded clips, in the
same target app (TextEdit).

## Usage

```sh
# live run (both apps, 3 passes, default hotkeys):
python3 scripts/bench/h2h.py --corpus evals/human-corpus --out /tmp/h2h-run

# one app, custom hotkey, longer settle:
python3 scripts/bench/h2h.py --app teletype --corpus DIR --passes 3 \
    --hotkey teletype="ctrl+cmd+space" --settle 3.0 --out /tmp/h2h-tele

# recompute metrics from an existing run:
python3 scripts/bench/h2h.py --metrics /tmp/h2h-run

# no hardware needed (CI / smoke):
python3 scripts/bench/h2h.py --demo --out /tmp/h2h-demo
```

Corpus format: `<clip>.wav` + `<clip>.txt` (expected text) per clip,
same layout as `scripts/eval/record_clip.sh` (use `.clean.txt` content
in `.txt` if mixing).

## Prerequisites

- macOS, terminal granted **Accessibility** permission
  (System Settings → Privacy & Security → Accessibility) for the
  System Events automation.
- Teletype installed with its dictation hotkey set to the chord passed
  via `--hotkey` (default `ctrl+cmd+space`).
- **Wispr Flow installed and signed in** for the `wispr` arm (default
  chord `shift+shift` — double-tap is not automatable; use Wispr's
  hold-style hotkey if you configure one).
- **Real-mic mode:** route the clip through a virtual mic (e.g.
  BlackHole) so `afplay` output reaches the dictation input, and set
  the virtual mic as the system input device.

## Method

- 3 passes per clip per app (more if the variance looks high).
- t_release stamped at chord release; t_text at first TextEdit text
  change (25 ms poll); failure = no text within 5 s.
- WER = unit Levenshtein over words vs `<clip>.txt`.
- Copy `WISPR_COMPARISON.md` (written to `--out`) into `evals/` and
  fill the versions table before publishing numbers.

## Honest limitations

- TextEdit is the only target app; per-app delivery differs in real use.
- 25 ms polling adds up to ~25 ms jitter to t_text.
- afplay startup latency is included in the timeline for both apps
  (fair, but not zero).
- Wispr's double-tap hotkey cannot be synthesized; the wispr arm
  requires a hold-style hotkey configured in Wispr Flow.
- Single machine, one mic path; results are not portable across
  hardware.
