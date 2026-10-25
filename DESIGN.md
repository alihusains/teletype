---
name: Teletype
description: Local-first voice dictation for a quiet, precise, on-device tool.
colors:
  # Light theme (shipped default)
  bg: "#f5f6f8"
  surface: "#ffffff"
  surface-hover: "#eef1f6"
  surface-2: "#f4f6f9"
  border: "#e6e9ef"
  border-subtle: "#eef0f4"
  text: "#141821"
  text-secondary: "#6b7280"
  text-tertiary: "#9aa1ad"
  accent: "#702fd2"
  accent-hover: "#5d24b5"
  accent-soft: "#f3edfd"
  accent-ink: "#5b21b6"
  on-accent: "#ffffff"
  danger: "#ef4444"
  danger-soft: "#fef3f2"
  success: "#16a34a"
  success-soft: "#e9fbf0"
  warning: "#b45309"
  warning-soft: "#fef4e6"
  scrim: "rgba(16, 20, 32, 0.45)"
  # Dark theme (first-class; border-led, not shadow-led)
  dark-bg: "#101014"
  dark-surface: "#1a1a22"
  dark-surface-hover: "#22222c"
  dark-surface-2: "#16161c"
  dark-border: "#2a2a34"
  dark-border-subtle: "#23232d"
  dark-text: "#ececf1"
  dark-text-secondary: "#9a9aa8"
  dark-text-tertiary: "#6f6f80"
  dark-accent: "#b69cff"
  dark-accent-hover: "#c9b4ff"
  dark-accent-soft: "#2a2140"
  dark-accent-ink: "#d3c4ff"
  dark-on-accent: "#0b1020"
  dark-danger: "#f87171"
  dark-success: "#4ade80"
  dark-warning: "#fbbf24"
  dark-scrim: "rgba(0, 0, 0, 0.65)"
typography:
  display:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif'
    fontSize: "28px"
    fontWeight: 650
    lineHeight: 1.15
    letterSpacing: "-0.025em"
  headline:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif'
    fontSize: "20px"
    fontWeight: 650
    lineHeight: 1.3
    letterSpacing: "-0.2px"
  title:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif'
    fontSize: "15px"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "-0.2px"
  body:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif'
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "normal"
  label:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif'
    fontSize: "12px"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "0.07em"
rounded:
  xs: "6px"
  sm: "8px"
  md: "12px"
  full: "999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "20px"
  xxl: "28px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.sm}"
    padding: "6px 12px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.sm}"
    padding: "6px 12px"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
    padding: "6px 12px"
  button-ghost:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text-secondary}"
    rounded: "{rounded.sm}"
    padding: "6px 12px"
  card:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
    padding: "20px"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
    padding: "8px 10px"
  chip:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text-tertiary}"
    rounded: "{rounded.full}"
    padding: "2px 8px"
---

# Design System: Teletype

## Overview

**Creative North Star: "The quiet instrument"**

Teletype is a precision tool for turning speech into clean, private, on-device
text. The interface behaves like a well-made instrument: calm neutral surfaces,
tight typographic hierarchy, and one violet accent that only fires when you act.
It is not a marketing page and not a colorful dashboard. It is a settings-heavy,
settings-first application where the user's own text, metrics, and model state
are the content, and the chrome around them recedes.

The personality is **refined and restrained**: crisp hairline borders, small
radii, tight padding, and low-contrast chrome. Interactive elements earn their
elevation; a surface lifts (a soft shadow and a 1px rise) only on hover, and a
focus ring appears only for keyboard users. Density is comfortable rather than
sparse: rows, sections, and cards stack on a 4px rhythm so a screen can hold a
lot of control without feeling busy.

Both a light and a dark theme are first-class. Light is the shipped default
(`data-brand="purple"` on a light palette); dark is applied when the app forces
it or the OS prefers it and the setting is not pinned to light. The dark theme
is **border-led**, not shadow-led: on near-black surfaces drop shadows fade to
invisibility, so depth moves to a slightly stronger hairline border instead.

**Key Characteristics:**
- One chromatic accent (Teletype Violet) reserved for action, selection, and focus.
- Flat, hairline-bounded cards that lift on interaction rather than resting in shadow.
- A system font stack with a short 12 to 28px scale; no display type above 28px.
- Comfortable density on a 4px spacing rhythm; the whole row is the target.
- Light and dark are both real themes, dark carried by borders not shadows.

## Colors

A near-monochrome neutral system in both themes, with a single violet accent
doing all the chromatic work. Semantic colors (danger, success, warning) are
used sparingly for state, never for decoration.

### Primary
- **Teletype Violet** (`#702fd2` light / `#b69cff` dark): the one action color.
  Primary buttons, the selected rail row, toggle-on fill, focus rings, active
  states, and the selected icon tile. It is the whole brand voice.
- **On-accent** (text on a violet fill): white (`#ffffff`) in light, where the
  accent is dark enough to carry it; near-black ink (`#0b1020`) in dark, where
  the accent lightens to `#b69cff` and white text would fail contrast. The
  primary-button label flips to match.

### Secondary
There is no secondary accent. The system is one-voice by design; the semantic
colors below are state colors, not a palette family.

### Neutral (light)
- **Canvas** (`#f5f6f8`): page background.
- **Surface** (`#ffffff`): cards, panels, and the default button fill.
- **Surface 2** (`#f4f6f9`): inset fields, unselected segmented control, chip and
  icon-tile backgrounds.
- **Surface Hover** (`#eef1f6`): hover fill on buttons and interactive rows.
- **Text** (`#141821`): primary text and headings.
- **Text Secondary** (`#6b7280`): subtext, screen subtitles, hints.
- **Text Tertiary** (`#9aa1ad`): eyebrows, placeholders, taglines, de-emphasized labels.
- **Border** (`#e6e9ef`): the 1px stroke on cards, inputs, and controls.
- **Border Subtle** (`#eef0f4`): hairline dividers between stacked rows.
- **Scrim** (`rgba(16, 20, 32, 0.45)`): the veil behind dialogs and popovers.

### Neutral (dark)
- **Canvas** (`#101014`): page background.
- **Surface** (`#1a1a22`): cards and panels.
- **Surface 2** (`#16161c`): inset fields and icon tiles.
- **Surface Hover** (`#22222c`): hover fill.
- **Text** (`#ececf1`): primary text.
- **Text Secondary** (`#9a9aa8`) / **Text Tertiary** (`#6f6f80`): subtext and labels.
- **Border** (`#2a2a34`) / **Border Subtle** (`#23232d`): the stronger hairline that
  carries depth where shadows disappear.
- **Scrim** (`rgba(0, 0, 0, 0.65)`): dialog veil.

### Semantic
- **Danger** (`#ef4444` light / `#f87171` dark): destructive actions, errors.
  Soft fill `#fef3f2` / `rgba(248, 113, 113, 0.12)`.
- **Success** (`#16a34a` light / `#4ade80` dark): confirmed, ready states.
  Soft fill `#e9fbf0` / `rgba(74, 222, 128, 0.12)`.
- **Warning** (`#b45309` light / `#fbbf24` dark): a model that needs downloading,
  a setting that applies next take. Distinct from danger on purpose; a routine
  first-run step is not an error. Soft fill `#fef4e6` / `rgba(251, 191, 36, 0.12)`.

### Named Rules
**The One-Voice Rule.** Teletype Violet is the only chromatic action color, and it
is used on a small fraction of any screen. Its rarity is the point; flooding a
surface with purple erases the hierarchy that the accent is there to create.
**The Border-Led Dark Rule.** In dark mode, do not reach for drop shadows to show
depth. Raise the border's presence instead, because shadow is nearly invisible on
a near-black surface and a dark UI built on shadow alone reads as flat greys.

## Typography

**Display Font:** System UI stack (SF Pro on macOS, Segoe UI on Windows)
**Body Font:** Same stack
**Label Font:** Same stack; uppercase for eyebrows only

**Character:** One system typeface everywhere, from a 28px screen title down to a
12px label. Hierarchy is built from size and weight, not from a second typeface.
Display sizes take a touch of negative tracking so headings read as tight,
confident blocks rather than loose text.

### Hierarchy
- **Display** (650, 28px, 1.15, -0.025em): the screen title. One per screen, at the top.
- **Headline** (650, 20px, 1.3, -0.2px): section titles inside a screen.
- **Title** (600, 15px, 1.4, -0.2px): settings section headers and card titles.
- **Body** (400, 15px, 1.5): default text. The app's working size.
- **Body Small** (400, 13.5px): row labels and denser list text.
- **Label** (600, 12px, 0.07em, UPPERCASE): the eyebrow. Establishes the
  label/value hierarchy above a value; tertiary color.

### Named Rules
**The No-Heavy-Type Rule.** Weight tops out at 650 for display and 600 for body.
There is no 700-plus display slab; confidence comes from size and tracking, not
from weight.

## Layout

A single centered content column, capped at 1200px. The app is a desktop
(primarily macOS and Windows) settings surface: a screen has a title and an
optional subtitle at the top, then a stack of sections. Rail/detail pages (the
Dictionary model) pair a fixed 216px left rail card with a scrolling content
pane, and the rail never moves while the pane scrolls.

Density is comfortable. Everything sits on a 4px rhythm: 4px between tight
elements, 8px inside controls, 12px between related items, 16px between groups,
20px of card padding, and 28px between major sections. Rows are full-width
hit targets, roughly 44px tall, so a 24px switch is never the thing you actually
aim for. Wide windows keep the extra room on the right so titles and first
controls share one left origin across every tab.

## Elevation & Depth

The system is **lifted, not floating**: cards are flat and hairline-bounded at
rest, and elevation is a response to state. Three shadow steps exist, ambient
rather than drop, and in dark mode the shadow vocabulary is deliberately
suppressed in favor of a stronger border.

### Shadow Vocabulary
- **Step 1** (`0 1px 2px rgba(16,20,32,0.05)`): the resting card. Barely there; the
  border does most of the work. Dark: `0 1px 2px rgba(0,0,0,0.3)`.
- **Step 2** (`0 1px 2px rgba(16,20,32,0.05), 0 4px 12px rgba(16,20,32,0.06)`):
  hover and popovers (the explainer popover, a lifted interactive card). Dark:
  `0 1px 3px rgba(0,0,0,0.36)`.
- **Step 3** (`0 2px 4px rgba(16,20,32,0.06), 0 12px 32px rgba(16,20,32,0.1)`):
  dialogs and the highest surfaces. Dark: `0 8px 28px rgba(0,0,0,0.5)`.
- **Focus Ring** (`0 0 0 3px color-mix(in srgb, var(--accent) 30%, transparent)`):
  keyboard focus only, via `:focus-visible`. Dark uses 42% mix.

### Named Rules
**The State-Lift Rule.** Surfaces rest flat. A shadow and a 1px upward transform
appear only on hover; the focus ring appears only for keyboard users. Nothing
sits in shadow at rest, and dark mode raises the border instead of the shadow.

## Shapes

Corners are small and consistent. The language is a rounded rectangle with a
1px hairline, not a soft blob. Radius comes in four steps: 6px for tiny chips and
badges, 8px for controls (buttons, inputs, small fields), 12px for cards and
panels, and 999px (full) for pills, chips, and toggle tracks. Borders are always
a single 1px hairline in the theme's border color; there are no multi-pixel
strokes or hard edges. Icon tiles are small squares at 8 to 9px radius.

## Components

The feel is **refined and restrained**: quiet borders, small radii, tight
padding. The row is the unit of the settings surface, and the whole row is the
target.

### Buttons
- **Shape:** 8px radius, 6px 12px padding, weight 500, 13.5px text.
- **Primary:** Teletype Violet fill, `--on-accent` text, no border. The one
  affirmative action in a group. Hover shifts to `--accent-hover`.
- **Secondary:** transparent fill, 1px `--border` stroke, `--text` color. Every
  button that is not the row's primary. Destructive actions are Secondary with
  the danger color and danger border, so they never read as the affirmative next
  to it.
- **Ghost:** transparent, no border, `--text-secondary` text; hover fills
  `--surface-hover`. Low-priority and navigation actions.
- **Small:** 4px 10px padding, 12px text, 6px radius, for dense in-card actions.

### Inputs / Fields
- **Style:** 8px radius, 1px `--border`, 8px 10px padding, `--surface` background.
- **Focus:** border moves to the accent and the focus ring appears.
- **Placeholder:** `--text-tertiary`, so a placeholder never competes with a value.
- **Disabled:** 50% opacity.

### Cards / Containers
- **Corner:** 12px radius, 1px `--border-subtle` (or `--border`), Step 1 shadow.
- **Background:** `--surface`.
- **Padding:** 20px.
- **Interactive:** hover adds Step 2 shadow, moves the border to `--border`, and
  lifts 1px via transform only (compositor-safe, no layout).

### Toggle
- 40x24px track, full radius, 1px border. On: violet fill; off: `--surface-2`.
  The 18px thumb is `--bg` with a Step 1 shadow and slides 16px.

### Segmented Control
- 2px inset padding, `--surface-2` track, 1px border, 8px radius. The active
  segment is `--surface`, 600 weight, `--text`, Step 1 shadow; inactive is
  transparent with `--text-tertiary`.

### Chip
- Full radius, transparent fill, 1px tinted border, 12px text. Read-only state
  markers in neutral, success, warning, or danger. Never used to imply an action
  the user cannot take.

### Rail (Dictionary model)
- 216px fixed card, 1px border, 12px radius. Each row is a 32px icon tile plus a
  14px label and an 11.5px tertiary tagline. The selected row gets a 1.5px
  accent border, an `--accent-soft` fill, a violet icon tile, and a violet label.

### Explainer ("?")
- A 16px circular tertiary affordance beside a section title. Expands to a 260px
  popover (`--surface-2`, Step 2 shadow) that keeps a paragraph of explanation out
  of everyone's face.

### The Floating Pill (signature, isolated)
The floating pill is the one surface that deliberately does not wear the app's
token system. It is always dark (a near-black `rgba(24,24,32,0.96)` shell), and
it owns its own colors: a red recording state (`#ff453d` / `#e5484d`), a cyan
activity dot (`#1FD5F9`, borrowed from LiveKit), and a yellow command tag
(`#f5c518`). It uses white-at-alpha text and tabular-nums monospace for
timings. Do not re-token-style the pill to match the app palette; its dark,
self-contained look is the reference (see the reverted 2026-10-20 pill revamp).
The pill preview stages in settings (`data-preview-theme`) are separate mock
palettes for the preview surface only, not the app's tokens.

## Do's and Don'ts

### Do:
- **Do** reserve Teletype Violet for action, selection, focus, and active state
  only; keep it on a small fraction of any screen.
- **Do** build new settings from the shared primitives (Section, Row, Toggle,
  Segmented, Select, Chip, buttons) so a new section cannot drift into a new look.
- **Do** make the whole row the click target, and keep interactive rows about 44px tall.
- **Do** use the 12px radius for cards, 8px for controls, and 999px for pills and chips.
- **Do** lift on hover with Step 2 shadow plus a 1px transform, and show the focus ring only via `:focus-visible`.
- **Do** carry dark-mode depth with a slightly stronger border, not a bigger shadow.
- **Do** use warning (amber) for "needs a download" and "applies next take"; reserve danger red for real errors and destructive actions.

### Don't:
- **Don't** introduce a second chromatic accent; violet is the only action color.
- **Don't** push type weight above 650 (display) or 600 (body), or display size above 28px.
- **Don't** rest a surface in shadow; elevation is a response to hover, focus, or a raised layer.
- **Don't** let placeholder or tertiary text do the reading; it is for labels and hints only.
- **Don't** style the floating pill with the app's tokens; it is a dark, isolated signature surface.
- **Don't** use multi-pixel borders, hard square corners on cards, or glassmorphism; the language is a 1px hairline and a small radius.
