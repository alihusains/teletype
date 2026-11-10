# t1-date: Comprehensive Date Format Dropdown for AutoText

## Status: COMPLETE

## Summary

Added a date format dropdown to the AutoText screen's "How triggers work" settings section, with backend support for rendering named and custom date format templates.

## Changes

### Backend: `crates/teletype-core/src/autotext/placeholders.rs`

- **`date_format_template(id: &str) -> Option<&'static str>`** — Maps named format IDs to template strings:
  - `""` / `"iso"` → `YYYY-MM-DD`
  - `"us"` → `MM/DD/YYYY`
  - `"us-short"` → `MM/DD/YY`
  - `"eu"` → `DD/MM/YYYY`
  - `"eu-short"` → `DD/MM/YY`
  - `"long"` → `D MMM YYYY`
  - `"long-us"` → `MMMM D, YYYY`
  - `"dmy-dash"` → `DD-MM-YYYY`
  - `"ymd-slash"` → `YYYY/MM/DD`
  - `"mdy-dash"` → `MM-DD-YYYY`
  - `"dmy-dash-short"` → `DD-MM-YY`

- **`render_date_template(template: &str, now: i64) -> String`** — Renders a template against the local date. Supports tokens (longest-first): `YYYY`, `YY`, `MMMM` (full month name), `MMM` (abbreviated), `MM` (zero-padded), `M`, `DD` (zero-padded), `D`. Unrecognized characters pass through as literal separators. A template with no recognized tokens is returned verbatim.

- **Unit tests** (20 passing): covers all named IDs, zero-padding, 2-digit years, month names, custom templates, and the fallback-to-verbatim behavior.

### UI: `ui/src/screens/AutoTextScreen.tsx`

- Added `DATE_FORMATS` constant listing all named formats with human-readable labels.
- Added `dateFormat` to the `Settings` interface.
- Added a **"Date format"** row in the "How triggers work" `Section`, after "Expand timing":
  - A `<select>` dropdown listing all named formats plus "Custom…"
  - When a non-named value is selected (or "Custom…"), a monospace free-text input appears for the template.
  - Saves via the existing `save_settings` IPC command (the `date_format` field already existed in the backend settings struct).
  - Hold-and-restore pattern matching the other settings rows (optimistic patch, rollback on failure).
  - `WriteFailed` card with retry on save rejection.

## Verification

- `cargo test -p teletype-core placeholders` — **20 passed**
- `npx tsc --noEmit` — **0 errors**
- `npx vitest run` — **543 passed (52 files)**

## File Ownership

- `crates/teletype-core/src/autotext/placeholders.rs` (backend rendering)
- `ui/src/screens/AutoTextScreen.tsx` (UI dropdown)

## Notes

- The `date_format` field already existed in `crates/teletype-desktop/src/commands.rs` settings struct (line 586) with `#[serde(default)]` — no backend wiring was needed beyond the core rendering functions.
- The pipeline already reads `date_format` from settings and passes it through to `expand_snippets_with` / `protect_snippets_with`.
