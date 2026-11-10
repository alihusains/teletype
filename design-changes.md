# Teletype — UI/UX Technical Audit (`/impeccable audit`)

**Date:** 2026-11-09
**Scope:** Full UI/UX technical quality audit of the Teletype app UI.
**Method:** Rendered evidence via the browser-preview shim + dedicated Chromium (CDP on `[::1]:9222`), scanned with `impeccable detect`, plus AX-tree, keyboard-focus, contrast, bundle, and asset checks.

> **Method note.** Tauri on macOS (WKWebView) exposes no CDP, so rendered evidence was obtained through the app's own browser-preview shim: the Vite dev server (port 1420) was loaded in a dedicated Chromium instance (CDP on `[::1]:9222`), and every screen was rendered, screenshotted, and scanned with the `impeccable detect` CDP scanner. Screens covered: all 13 nav screens in light mode, Home/Settings/Dictation in dark mode, Home at 700px narrow, and the onboarding wizard. AX-tree, keyboard-focus, contrast, bundle, and asset checks were all run against the rendered page, not inferred from source.

---

## Audit Health Score

| # | Dimension | Score | Key Finding |
|---|-----------|-------|-------------|
| 1 | Accessibility | **2** | Focus ring lost on all primary buttons (WCAG 2.4.7); history rows are `role="button"` with no activation handler and nested buttons |
| 2 | Performance | **3** | Lean bundle (99 kB gzip main); ~2.95 MB of dead assets shipped in `ui/public/` |
| 3 | Theming | **3** | Token-driven, dark mode coherent and contrast-clean; 43 hard-coded color values remain (mostly pill glow system) |
| 4 | Responsive Design | **3** | Narrow icon-rail mode works; touch targets below the 24px spacing exception (28×28 icons, 2px gap) |
| 5 | Implementation Integrity | **1** | `DESIGN.md` documents a design system that no longer ships — systemic doc/implementation drift |
| **Total** | | **12/20** | **Acceptable (significant work needed)** |

---

## Implementation Integrity Verdict

**Start here.** **Partial pass.** The shipped UI is internally coherent: all 15 screens express one consistent "Letters" system (Open Runde + Inter, shared `--token` set, one button/row/card language, a coherent dark theme). The implementation is not interchangeable with an unrelated product, and the detector's structural findings (nested-cards, ai-color-palette) fall on the pill-preview stage, which is a documented exception.

However, the **design-system documentation is systemically stale**: `DESIGN.md` (written 2026-10-26, `dbc2272`) describes system fonts, 12px radii, violet `#702fd2` accents, flat shadows, and a `#f5f6f8` background — none of which ship. The Open Runde/"Letters" redesign landed 2026-10-29 (`8e392c1`) and the doc was never updated; its frontmatter token values disagree with `ui/index.html` in most fields. An agent or designer working from `DESIGN.md` would "fix" the real system toward a dead one. This is recorded as brain gotcha **G032**.

---

## Executive Summary

- Audit Health Score: **12/20 (Acceptable — significant work needed)**
- Total issues: **12** — P0: 0 · P1: 4 · P2: 4 · P3: 4
- Top issues:
  1. **All primary buttons lose their focus ring** when keyboard-focused (WCAG 2.4.7) — verified: `Start dictating` matches `:focus-visible` but its computed `box-shadow` is the resting shadow, because `.tt-btn-primary`'s resting shadow wins on source order.
  2. **Home history rows are keyboard dead-ends**: `div role="button" tabIndex={0}` with no click/keydown handler, and they *contain* the Copy/Delete buttons (invalid ARIA nesting; the row's accessible name swallows "Copy to clipboard Delete entry").
  3. **Light-theme contrast failures** across nav labels, placeholders, success text, and the Home hero (worst: 1.5:1 on the hero gradient).
  4. **The browser-preview shim crashes the whole app** (`ModelsScreen` reads `m.purposes.map()`; the shim's fixtures lack `purposes`). Dev-tooling only — the Rust catalog always sends the field and the shim is tree-shaken from production — but it blocks the entire preview workflow.
- Recommended next steps: run the P1 items first (all small, high-leverage fixes), then regenerate `DESIGN.md` from the shipped system.

---

## Detailed Findings by Severity

### P1 — Major

**[P1] Focus ring lost on all primary/filled buttons**
- **Location:** `ui/index.html` — `.tt-btn-primary` / `button.primary` rule (~line 391) vs the `:focus-visible` rule (~line 330)
- **Category:** Accessibility
- **Impact:** Keyboard users cannot tell which action button is focused. Every filled CTA in the app ("Start dictating", "Download", "Use", primary rows) is affected.
- **WCAG/Standard:** WCAG 2.4.7 Focus Visible (A) — fails.
- **Evidence:** Real Tab navigation in Chromium: `document.activeElement` = `button.tt-btn.tt-btn-primary`, `matches(':focus-visible')` = true, computed `box-shadow` = the resting 4-layer gray shadow, `outline` = `none`.
- **Recommendation:** Make the focus ring win: scope the ring as `.tt-btn:focus-visible` (specificity 0-2-0 beats `.tt-btn-primary`'s 0-1-0) or append the ring shadow to the primary's own `:focus-visible` rule.
- **Suggested command:** `/impeccable harden`

**[P1] Home history rows: fake button, nested buttons, keyboard dead-end**
- **Location:** `ui/src/screens/HomeScreen.tsx` lines 486–488 (`HistoryRow`)
- **Category:** Accessibility
- **Impact:** Screen-reader users hear a button whose name includes "Copy to clipboard Delete entry" (the nested buttons bleed into the name); pressing Enter/Space does nothing (no handler); the row's `tabIndex={0}` puts a non-functional control in the tab order.
- **WCAG/Standard:** WCAG 4.1.2 Name/Role/Value (A) — nested interactive content inside a button is invalid ARIA; WCAG 2.1.1 for the no-op key handling.
- **Recommendation:** If the row is not actually activatable, drop `role="button"` and `tabIndex` (make it a plain `li`-style row; the real actions are the named Copy/Delete buttons, which already work). If a row action is wanted, add `onClick`/`onKeyDown` and move Copy/Delete out of the button's DOM subtree.
- **Suggested command:** `/impeccable harden`

**[P1] Light-theme contrast failures (dark theme is clean)**
- **Location:** `ui/index.html` tokens + `HomeScreen.tsx` hero
- **Category:** Accessibility
- **Impact:** Low-vision users cannot read nav group labels, date separators, inactive segment options, placeholders, success states, or the Home hero subtext.
- **WCAG/Standard:** WCAG 1.4.3 Contrast (AA) — fails.
- **Measured (detector, rendered):** `#8b8b8b` on `#ffffff` = **3.4:1** (nav group labels, TODAY/YESTERDAY, inactive segments); `#8b8b8b` on `#f5f5f5` = **3.1:1** (placeholders); `#16a34a` on `#ffffff` = **3.3:1** ("Ready", "In use", "Up 25%"); hero white/0.9 text over the light gradient end `#cbdcec` = **1.5:1–2.1:1** (subtext + eyebrow).
- **Recommendation:** Darken tertiary text to ≈ `#6b6b6b` (4.6:1 on white) or `#5f5f5f`; success green to `#15803d` (4.6:1); hero subtext/eyebrow to a solid dark ink or raise the gradient's dark end.
- **Suggested command:** `/impeccable harden` (contrast) — or `/impeccable colorize` for the hero treatment.

**[P1] Browser-preview shim crashes the entire app in preview mode**
- **Location:** `ui/src/dev/browserPreview.ts` (`list_speech_models` fixtures) × `ui/src/screens/ModelsScreen.tsx:813` (`m.purposes.map`)
- **Category:** Implementation Integrity (dev tooling)
- **Impact:** Any developer opening the Vite URL in a browser gets the error boundary instead of the app — every screen is unviewable in preview, which is the repo's own review workflow (screenshots, this audit).
- **WCAG/Standard:** n/a
- **Evidence:** CDP `Runtime.exceptionThrown`: `TypeError: Cannot read properties of undefined (reading 'map')` at `ModelsScreen.tsx` (render of `ModelsScreen`). The Rust catalog (`crates/teletype-speech/src/catalog.rs`) always serializes `purposes`, and both test fixtures include it — only the shim is missing the field. **No production impact** (shim tree-shaken; verified 0 shim strings in the prod bundle).
- **Recommendation:** Add `purposes` to the four `list_speech_models` fixtures in the shim (matching the `SpeechPurposeStatus` shape: `label`, `basis`, `evidence`).
- **Suggested command:** direct code fix (one-line-per-fixture); no design command needed.

### P2 — Minor

**[P2] Functional text below the 11px legibility floor**
- **Location:** nav group labels + TODAY/YESTERDAY (10.5px); Insights percentages (10px); pill preview "LISTENING" (9.5px)
- **Category:** Accessibility / Typography
- **Impact:** These are content-bearing labels (dates, stats), not legal smallprint; at 10–10.5px they blur on high-DPI displays and degrade tap/read targets.
- **Recommendation:** Move to the 11px+ ramp step; if the nav labels must stay visually quiet, fix contrast (P1) and go to 11px with the existing tracking.
- **Suggested command:** `/impeccable typeset`

**[P2] No h1; screen titles are h2; Settings mixes h2/h3**
- **Location:** all screens (`.tt-screen-title` is `<h2>`); `SettingsScreen` (h2 sections + h3 group labels, h3 at 12px vs h2 at 15px → flat hierarchy, flagged by the `flat-type-hierarchy` rule)
- **Category:** Accessibility / Semantics
- **Impact:** Screen-reader landmarks/navigation ("jump to heading") starts at level 2 on nearly every screen; Settings' heading outline misrepresents its structure.
- **Recommendation:** Make the visible screen title an h1 (one per screen — they are separate views), keep section headings at h2; in Settings, demote group labels to non-heading text or promote sections consistently.
- **Suggested command:** `/impeccable typeset`

**[P2] `DESIGN.md` is fully stale vs the shipped "Letters" system**
- **Location:** `/DESIGN.md` (frontmatter + body) vs `ui/index.html`
- **Category:** Implementation Integrity
- **Impact:** Anyone (human or agent) designing or auditing from the doc works against a dead system: fonts (system vs Open Runde+Inter), radii (12px vs 18px), accent (violet vs obsidian), shadows (flat vs blue-tinted), bg (`#f5f6f8` vs `#ffffff`), and the sky hero is absent from the doc.
- **Evidence:** git dates (`dbc2272` 2026-10-26 doc vs `8e392c1` 2026-10-29 redesign) + rendered comparison of all 13 screens against the doc's claims.
- **Recommendation:** Regenerate `DESIGN.md` from the shipped `ui/index.html` token block and the actual screens.
- **Suggested command:** `/impeccable document`

**[P2] ~2.95 MB of dead assets shipped in the app bundle**
- **Location:** `ui/public/` — `teletype-icon.png` (916K), `teletype-icon-white.png` (784K), `home-hero.jpeg` (404K), `autotext-hero.jpeg` (484K), `home-card.png` (360K)
- **Category:** Performance
- **Impact:** Every install/bundle carries ~3 MB of images referenced nowhere (verified: zero references in `ui/src`, `index.html`, `pill.html`, or `crates/`); they also land in `dist/` on every build.
- **Recommendation:** Delete the five files (keep the 20K `teletype-app-icon-{white,purple}.png` pair, which is used).
- **Suggested command:** `/impeccable optimize`

### P3 — Polish

**[P3] Touch targets below the WCAG spacing exception**
- **Location:** nav rows 212×**37px**; Copy/Delete icon buttons **28×28** with a 2px gap
- **Category:** Responsive
- **Impact:** Desktop-first, so impact is low, but a 24px-circle around each 28px icon overlaps its neighbor (the WCAG 2.5.8 spacing exception fails) — a precision issue for trackpad/coarse-pointer users.
- **Recommendation:** Bump icon buttons to 32×32 with ≥8px separation, or accept 28px and widen the gap.
- **Suggested command:** `/impeccable layout`

**[P3] Insights straggler words + nav scroll + missing roving**
- **Location:** Insights proof lines (`"counted."` alone on the last of 3 lines, 13% of measure); nav (Settings requires scrolling at normal window height); `ui/src/settings/primitives.tsx` `Segmented`
- **Category:** Typography / Responsive / Keyboard
- **Impact:** Cosmetic rag; the last nav item is not visible without scrolling; the radiogroup has no arrow-key roving (click works; keyboard users must Tab through each option).
- **Recommendation:** Tighten the proof copy or line length; check nav vertical budget at the minimum window height; add `onKeyDown` left/right handling to `Segmented`.
- **Suggested command:** `/impeccable polish`

**Detector false positives (verified, not issues):** `overused-font` (Inter at 89% — it is the intentional body face); `dark-glow` (the zero-offset `#702fd2` shadow is the intentional `pulseGlow`); `ai-color-palette` / `nested-cards` (pill-preview stage, a documented exception).

---

## Patterns & Systemic Issues

- **Contrast debt is concentrated in one token:** `#8b8b8b` (tertiary gray) and `#16a34a` (success green) fail on white in every light screen where they appear — fixing two token values resolves most of the 13 detector contrast findings.
- **The design doc and the design system diverged on a single commit boundary:** the 2026-10-29 redesign (`8e392c1`) changed fonts, radii, accent, shadows, and hero treatment in one pass without touching `DESIGN.md`. There is no doc-refresh gate on UI redesigns — the same drift will recur on the next redesign.
- **Preview fixtures drift from the real IPC contract:** the shim's `list_speech_models` predates the `purposes` field that both the Rust catalog and the test fixtures carry. The shim is the only place where a missing field crashes the app, so it needs either a shared fixture source or a render-smoke test.
- **Interactive-looking containers without behavior:** the history-row `role="button"` is the one place the app's otherwise-clean semantics pattern (real `<button>`s, named switches/sliders/comboboxes, landmarks) breaks.

---

## Positive Findings

- **Every interactive element has an accessible name** — 0 unnamed buttons/links/checkboxes/radios/sliders/comboboxes/textboxes across the full rendered AX tree (1,800 nodes on Settings alone); switches, sliders, and comboboxes are properly exposed.
- **Landmarks present** (`nav` + `main`) and the focus ring works on every non-primary control, verified with real Tab navigation.
- **Dark mode is coherent and contrast-clean** — the detector found zero contrast failures in dark theme; it is a first-class theme, not an afterthought.
- **Reduced motion is handled** (in-app toggle + OS preference) without a destructive global kill.
- **Lean, honest bundle:** main chunk 390 kB → **99 kB gzip**; the preview shim is properly tree-shaken from production (verified 0 shim strings in the built bundle); fonts are bundled locally (no network).
- **Test culture for a11y:** a `vitest-axe` suite covers the main screens, and an error boundary turns render crashes into readable messages.
- **Narrow-window icon-rail mode works** (700px verified, no horizontal overflow) and the onboarding wizard renders end-to-end.

---

## Recommended Actions

1. **[P1] `/impeccable harden`**: restore the `:focus-visible` ring on `.tt-btn-primary`/`button.primary`, fix the history-row ARIA (remove the fake button role or give it real activation, and un-nest Copy/Delete), and raise the failing light-theme contrast (`#8b8b8b` → ≈`#6b6b6b`, `#16a34a` → `#15803d`, hero subtext/eyebrow to dark ink).
2. **[P1] direct fix**: add `purposes` to the four `list_speech_models` fixtures in `ui/src/dev/browserPreview.ts` so the preview stops crashing on `ModelsScreen`.
3. **[P2] `/impeccable document`**: regenerate `DESIGN.md` from the shipped `ui/index.html` token system and the current screens (the doc currently describes a design that no longer ships).
4. **[P2] `/impeccable typeset`**: bring 10–10.5px functional labels up to the 11px floor, make screen titles h1, and fix Settings' h2/h3 flat hierarchy.
5. **[P2] `/impeccable optimize`**: delete the five unreferenced ~2.95 MB assets in `ui/public/`.
6. **[P3] `/impeccable layout`**: enlarge adjacent 28px icon buttons (or their gap) to meet the WCAG 2.5.8 spacing exception, and check the nav's vertical budget so Settings is visible without scrolling.
7. **[P3] `/impeccable polish`**: fix the Insights straggler words and add arrow-key roving to the `Segmented` control.

> You can ask me to run these one at a time, all at once, or in any order you prefer.
>
> Re-run `/impeccable audit` after fixes to see your score improve.

---

## Evidence & Artifacts

- **Screenshots (rendered):** `/private/var/folders/w3/y5xsfv8x42v6qcn7gdmdt769hgrqq5/T/opencode/teletype-audit-shots/`
  - `home-light.png`, `light-{insights,dictation,notetaker,transforms,autotext,dictionary,style,command-mode,personalization,models,keybinds,settings}.png`
  - `dark-home.png`, `dark-settings.png`, `dark-dictation.png`
  - `narrow-home.png` (700px), `onboarding.png`, `focus-primary.png`
- **Detector JSON (rendered CDP scans):** `detect-{home,insights,settings,dark-home}.json` (removed after the audit; findings captured above)
- **Source of truth for the design system:** `ui/index.html` (the `:root` / `[data-theme="dark"]` token blocks + `.tt-*` classes) — **not** `DESIGN.md` (see brain gotcha G032).

## Cleanup / Knowledge Review (per AGENTS.md)

- Reverted the 4 temporary `purposes` edits in `ui/src/dev/browserPreview.ts` (verified: `git diff` clean, 0 `purposes` lines remain) — the P1 shim crash is documented, not fixed, as the audit requires.
- Killed the audit Chrome instance (dedicated user-data-dir on `[::1]:9222`); the user's Brave on 127.0.0.1 was untouched. Removed temp scripts/JSON; screenshots kept as evidence.
- Project brain updated: new **Gotcha G032** (DESIGN.md stale vs shipped Letters system, with git-date evidence and the rule to treat `ui/index.html` as the design source of truth).
