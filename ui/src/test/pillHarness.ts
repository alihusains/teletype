// Shared harness for the pill layout regression suites
// (pillLayoutHigh/Medium/Low.test.tsx, PILL_SPEC.md section 11). Each test
// file still declares its own vi.mock for @tauri-apps/api; this module only
// stubs the browser APIs jsdom lacks and mounts the pill entry point.
import { expect, vi } from "vitest";
import { fireEvent, waitFor } from "@testing-library/react";
import {
  emitTauriEvent,
  invokeCalls,
  listenHandlersFor,
  mockInvokeResponses,
  mockListenCapture,
} from "./tauriMock";

class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
vi.stubGlobal("ResizeObserver", ResizeObserverStub);

// Frame callbacks are counted, never run: the tests assert which states
// schedule per-frame work, not the smoothing itself.
let rafCount = 0;
vi.stubGlobal("requestAnimationFrame", () => ++rafCount);
vi.stubGlobal("cancelAnimationFrame", () => {});

// jsdom has no matchMedia; the pill reads the OS reduced-motion preference.
// Each test can flip it before mounting.
let reducedMotion = false;
vi.stubGlobal("matchMedia", (media: string) => ({
  media,
  matches: /reduced-motion/.test(media) ? reducedMotion : false,
  addEventListener() {},
  removeEventListener() {},
}));

export function setReducedMotion(value: boolean): void {
  reducedMotion = value;
}

export function rafRequests(): number {
  return rafCount;
}

/** Call from afterEach (after cleanup and resetTauriMocks). */
export function resetPillHarness(): void {
  reducedMotion = false;
  rafCount = 0;
}

export type PillPhase =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs: number; command: boolean }
  | { phase: "warming" }
  | { phase: "processing"; message: string };

export function setPhase(phase: PillPhase): void {
  void emitTauriEvent<PillPhase>("pill-state", phase);
}

export const STYLES = ["default", "classic", "levelRail", "well", "dotGrid"] as const;

export async function renderPill(pillStyle: string, extra: Record<string, unknown> = {}): Promise<void> {
  mockInvokeResponses({
    get_settings: {
      recordingMode: "hold",
      language: "en",
      inputDevice: "",
      pillStyle,
    },
    list_input_devices: [],
    set_pill_size: undefined,
    ...extra,
  });
  mockListenCapture();
  document.body.innerHTML = '<div id="root"></div>';
  vi.resetModules();
  await import("../pillEntry");
  await waitFor(() => {
    expect(listenHandlersFor("pill-state")).toHaveLength(1);
  });
  // Settings load asynchronously after mount; the style branch depends on it.
  await new Promise((r) => setTimeout(r, 0));
}

export function root(): HTMLElement {
  return document.getElementById("root")!;
}

// Splits a box-shadow list on the commas between layers, not inside rgba().
function shadowLayers(value: string): string[] {
  return value
    .split(/,(?![^(]*\))/)
    .map((s) => s.trim())
    .filter(Boolean);
}

export function outerShadows(): string[] {
  const out: string[] = [];
  for (const el of root().querySelectorAll<HTMLElement>("*")) {
    const value = el.style.boxShadow;
    if (!value) continue;
    for (const layer of shadowLayers(value)) {
      if (!/\binset\b/.test(layer)) out.push(layer);
    }
  }
  return out;
}

export function lastPillSize(): { width: number; height: number } | undefined {
  const calls = invokeCalls().filter((c) => c.cmd === "set_pill_size");
  return calls[calls.length - 1]?.args as { width: number; height: number } | undefined;
}

export const AMBER_INSET = /inset\s+0(px)?\s+0(px)?\s+0(px)?\s+2px\s+rgba\(245,\s*197,\s*24/;

export function litDots(): string {
  const grid = root().querySelector<HTMLElement>("[data-dot-grid]")!;
  return [...grid.children]
    .map((el, i) => ((el as HTMLElement).style.opacity === "1" ? i : -1))
    .filter((i) => i >= 0)
    .join(",");
}

export const DEVICES = [
  { id: "mbp", name: "MacBook Pro Microphone", is_default: true },
  { id: "ext", name: "Shure MV7+ USB Podcast Microphone", is_default: false },
];

export function hoverPill(): void {
  // The hover target is the element that carries the pill's mouse handlers:
  // the measured wrapper (ported styles) or the capsule column (default).
  const target = root().firstElementChild!.firstElementChild as HTMLElement;
  fireEvent.mouseEnter(target);
}

export function textAlphas(): Array<{ text: string; alpha: number; struck: boolean }> {
  const out: Array<{ text: string; alpha: number; struck: boolean }> = [];
  for (const el of root().querySelectorAll<HTMLElement>("*")) {
    const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent!.trim());
    if (!own || !el.style.color) continue;
    const m = /rgba\([^)]*,\s*([\d.]+)\)/.exec(el.style.color);
    out.push({
      text: el.textContent!.trim(),
      alpha: m ? Number(m[1]) : 1,
      struck: el.style.textDecoration.includes("line-through"),
    });
  }
  return out;
}
