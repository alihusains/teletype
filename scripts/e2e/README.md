# E2E scripts (plan §9.6)

End-to-end checks for dictation delivery on a real macOS machine. These
run against the Teletype app with real apps in focus — they are NOT part
of `scripts/qa.sh` and are meant for a self-hosted runner (or a human
on a dev machine).

## The §9.6 matrix

| script | what it covers |
|---|---|
| `delivery_matrix_macos.sh` | target app × forced delivery fallback (`clipboard\|type\|setvalue\|hold`). Launches the app with `--e2e` and drives it through the unix-socket IPC hook; targets: TextEdit, Notes, Safari/Chrome (local test page), Terminal, Slack, Word, VS Code (if installed). Asserts the text lands and the clipboard contract holds. |
| `command_mode.sh` | command-mode cases: "open TextEdit" (app launch), "make this shorter" (selection edit). |
| `offline.sh` | full-network block via `pfctl` (rules in `offline_rules.txt`) around a command, to prove offline dictation works. Never auto-elevates; prints the sudo command for the operator. |

## Prerequisites

- macOS, terminal granted **Accessibility** permission (the script
  focuses targets and reads fields back; the app needs the same grant
  for AX delivery verification).
- `delivery_matrix_macos.sh` launches the app itself with `--e2e`; it
  needs no trigger env. `TELETYPE_E2E_BIN` points at the binary
  (default `target/debug/teletype`), `TELETYPE_E2E_WAV` overrides the
  fixture, and `TELETYPE_E2E_LOG` captures the app's stderr.
- `command_mode.sh` still requires `TELETYPE_E2E_TRIGGER`, a shell
  command that triggers one dictation; it passes the phrase as
  `TELETYPE_E2E_TEXT`.
- For `offline.sh`: a machine where `sudo pfctl` is allowed (CI
  self-hosted runner), and `offline_rules.txt` edited for your
  interface/subnet.
- Fixtures shorter than 1.2 s are padded with silence to 2.5 s: the app
  treats a Hold-mode release under 1 s as a quick tap (clipboard paste),
  not a dictation.

## How to run

```sh
# dry runs (no app, no trigger, no sudo) — must run green anywhere:
bash scripts/e2e/delivery_matrix_macos.sh --dry
bash scripts/e2e/command_mode.sh --dry
bash scripts/e2e/offline.sh --dry

# real runs (self-hosted runner or operator machine):
export TELETYPE_E2E_TRIGGER='...trigger one dictation...'
bash scripts/e2e/delivery_matrix_macos.sh
bash scripts/e2e/command_mode.sh
sudo bash scripts/e2e/offline.sh --run 'bash scripts/e2e/delivery_matrix_macos.sh'
```

`delivery_matrix_macos.sh` needs no trigger: it launches `--e2e` and
drives the socket itself. `command_mode.sh` exits 3 when
`TELETYPE_E2E_TRIGGER` is unset.

## Current limitations

- `command_mode.sh` still needs an external trigger command; the
  delivery matrix is fully socket-driven.
- Runs on the same machine as the app (no remote app under test).
- Field read-back is best-effort: TextEdit is read through its own
  AppleScript `text` property (its document is not an AX text area);
  Notes, Safari content, Chrome and Terminal may not expose a plain
  text area, so those cases accept the take's recorded text as the
  delivery proof.
- `keep_text_on_clipboard=true` intentionally leaves the dictation on
  the clipboard; the matrix follows the setting instead of always
  requiring the previous clipboard to be restored.
- Offline rules block `en0`/`en1`/`utun0` by default; edit
  `offline_rules.txt` for other interfaces (e.g. Wi-Fi on `en0` vs
  Thunderbolt ethernet).
