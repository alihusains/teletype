# E2E scripts (plan §9.6)

End-to-end checks for dictation delivery on a real macOS machine. These
run against the Teletype app with real apps in focus — they are NOT part
of `scripts/qa.sh` and are meant for a self-hosted runner (or a human
on a dev machine).

## The §9.6 matrix

| script | what it covers |
|---|---|
| `delivery_matrix_macos.sh` | target app × forced delivery fallback (`TELETYPE_FORCE_DELIVERY=clipboard\|type\|setvalue\|hold`). Targets: TextEdit, Notes, Safari (local test page), VS Code (if installed). Asserts text lands in the field and the clipboard is restored. |
| `command_mode.sh` | command-mode cases: "open TextEdit" (app launch), "make this shorter" (selection edit). |
| `offline.sh` | full-network block via `pfctl` (rules in `offline_rules.txt`) around a command, to prove offline dictation works. Never auto-elevates; prints the sudo command for the operator. |

## Prerequisites

- macOS, terminal granted **Accessibility** permission.
- Teletype app running, with the `TELETYPE_FORCE_DELIVERY` env hooks
  implemented (parallel core worker owns those).
- `TELETYPE_E2E_TRIGGER` set to a shell command that triggers one
  dictation (the e2e IPC hook). `command_mode.sh` additionally passes
  the phrase via `TELETYPE_E2E_TEXT`.
- For `offline.sh`: a machine where `sudo pfctl` is allowed (CI
  self-hosted runner), and `offline_rules.txt` edited for your
  interface/subnet.

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

Real runs exit 3 with "e2e IPC hook not wired yet" when
`TELETYPE_E2E_TRIGGER` is unset.

## Current limitations

- The e2e IPC trigger hook is **not wired yet** — real runs are pending
  the core worker's `TELETYPE_E2E_TRIGGER` support.
- Runs on the same machine as the app (no remote app under test).
- `read_field_text` is a best-effort AX read; some apps (Notes, Safari
  content) don't expose a plain `text area` and those cases may need
  per-target read commands.
- Offline rules block `en0`/`en1`/`utun0` by default; edit
  `offline_rules.txt` for other interfaces (e.g. Wi-Fi on `en0` vs
  Thunderbolt ethernet).
