# 221 — Impeccable audit: components in each tab

Status: ready-for-human

## Request
Run `/impeccable audit` over the components in each tab of the Teletype
settings window. Report the UX state and what can be changed. Audit only —
no fixes in this pass.

## Scope
All 14 navigable screens plus the shared primitives and the sidebar shell:
Home, Dictation, Insights, AI Polish (Transforms), AutoText, Dictionary,
Style, Command Mode, Notetaker, Personalization, Models, Settings, Keybinds,
Developer. Plus `settings/primitives.tsx`, `components/TabRail.tsx`,
`components/StateRows.tsx`, `components/DownloadProgress.tsx`,
`components/HotkeyRecorder.tsx`, and the `App.tsx` nav shell.

## Method
Source-level audit (Tauri on macOS = WKWebView, no CDP, so the rendered
detector did not run). Scanned for a11y, performance, theming, responsive
behavior, and implementation integrity against DESIGN.md and the Letters
token set in `ui/index.html`.

## Comments
Findings delivered in conversation 2026-11-08. See the audit report there.
