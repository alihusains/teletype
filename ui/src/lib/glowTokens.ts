// The "glow" pill style is adapted from OpenWhispr's VoicePill
// (github.com/OpenWhispr/openwhispr, src/components/dictation/VoicePill.tsx,
// PillWaveform.tsx, waveformMath.ts, src/helpers/voicePillPresentation.js,
// src/styles/dictation-panel.css). Copyright (c) 2024 OpenWhispr Team.
// MIT License. Modified: driven by Teletype's pill phases and the scalar
// `pill-level` stream, transform-based bars, glow confined inside the pill
// box, and a text note state that OpenWhispr does not have.
//
// Tokens, sizes and media-preference hooks shared by the glow pill files
// (ui/src/pillGlow.tsx, ui/src/pillGlowParts.tsx).

import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { RAINBOW } from "./rainbow";

// Literal values rather than imports from pill.tsx: pill.tsx imports the glow
// pill, so its constants are not initialised yet when this object is built.
export const GLOW = {
  surface: "rgba(20,20,28,0.82)", // DARK_SURFACE in pill.tsx
  surfaceOpaque: "rgb(20,20,28)", // prefers-reduced-transparency
  // The hairline border is an inset shadow, not a CSS border: it stays inside
  // the box and adds nothing to layout. (Chromium rounds a 0.5px border up to
  // 1px, so a real border would make the pill height differ by engine.)
  border: "inset 0 0 0 0.5px rgba(255,255,255,0.1)", // DARK_BORDER_W, DARK_BORDER
  borderContrast: "inset 0 0 0 1px rgba(255,255,255,0.35)", // prefers-contrast: more
  highlight: "inset 0 1px 0 rgba(255,255,255,0.06)",
  commandRing: "inset 0 0 0 2px rgba(245,197,24,0.55)",
  bar: "rgba(255,255,255,0.9)",
  mark: "rgba(255,255,255,0.7)",
  head: RAINBOW[5],
  bright: RAINBOW[6],
  core: RAINBOW[7],
  deep: RAINBOW[8],
  text: "rgba(255,255,255,0.92)",
  textSecondary: "rgba(255,255,255,0.62)",
  // Glyph only (non-text, 3:1). Error text stays primary: #ff6369 on the
  // composited surface over a light desktop measured 3.64:1, and 3.13:1 under
  // the error wash. The glyph and the wash carry the error tone.
  error: "#ff6369",
  warn: "#f5c518",
  success: RAINBOW[4],
  // Wash colours as [r, g, b, alpha]. More contrast makes the surface opaque
  // instead of doubling these (a doubled wash lowered text contrast).
  washSuccess: [0, 250, 154, 0.16],
  washWarn: [245, 197, 24, 0.14],
  washError: [255, 69, 61, 0.18],
  ease: "cubic-bezier(0.2, 0, 0, 1)",
} as const;

export const HEIGHT = 36;
export const RADIUS = HEIGHT / 2;
export const CAPSULE_W = 108;
export const CIRCLE_W = 36;
export const MORPH_MS = 300;
export const MORPH_FALLBACK_MS = 360;
export const FADE_MS = 160;
export const WORKING_HOLD_MS = 320;
// Staged entrance (OpenWhispr LISTENING_ENTRANCE_TIMING): hold the dot,
// widen, pause, then reveal the lane.
export const ENTRANCE_WIDEN_AT_MS = 260;
export const ENTRANCE_LANE_AT_MS = 660;
export const LANE_DIM = 0.45;
/** Cross-fades under reduced motion: kept, shortened (fades carry meaning). */
export const REDUCED_FADE_MS = 120;
/** Marks an element whose fade survives the reduced-motion rule in
 * pill.html, which otherwise cuts every animation and transition. */
export const FADE_ATTR = { "data-glow-fade": "" } as const;

export const RING_GRADIENT = `conic-gradient(from 0deg, transparent 0deg, ${GLOW.deep} 200deg, ${GLOW.core} 290deg, ${GLOW.bright} 340deg, ${GLOW.head} 356deg, transparent 360deg)`;
export const RING_MASK = "radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px))";

export const VISUALLY_HIDDEN: CSSProperties = {
  position: "absolute",
  width: 1,
  height: 1,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
};

// --- Media preference hooks --------------------------------------------------

function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() =>
    typeof window !== "undefined" && typeof window.matchMedia === "function" ? window.matchMedia(query).matches : false,
  );
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia(query);
    const onChange = (e: MediaQueryListEvent) => setMatches(e.matches);
    setMatches(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [query]);
  return matches;
}

export function usePrefersReducedTransparency(): boolean {
  return useMediaQuery("(prefers-reduced-transparency: reduce)");
}

export function usePrefersMoreContrast(): boolean {
  return useMediaQuery("(prefers-contrast: more)");
}

export interface Surface {
  background: string;
  /** The hairline border, as an inset box-shadow layer. */
  border: string;
}

/** The pill surface. More contrast also makes it opaque: over a light
 * desktop the translucent surface composites to about #3e3e45, which is what
 * held tinted text near 4.5:1. Memoised so the memoised glow parts keep it. */
export function useGlowSurface(): Surface {
  const transparencyOff = usePrefersReducedTransparency();
  const contrast = usePrefersMoreContrast();
  const opaque = transparencyOff || contrast;
  return useMemo(
    () => ({
      background: opaque ? GLOW.surfaceOpaque : GLOW.surface,
      border: contrast ? GLOW.borderContrast : GLOW.border,
    }),
    [opaque, contrast],
  );
}
