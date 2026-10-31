// claim: ui-behavior
// Regression tests for the existing-pill layout defects in
// research/pills/PILL_SPEC.md section 11 (H1-H5, M1-M8, L1-L6). Each describe
// block names the defect it pins.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, screen, waitFor } from "@testing-library/react";
import {
  emitTauriEvent,
  listenHandlersFor,
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
} from "./tauriMock";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
vi.stubGlobal("ResizeObserver", ResizeObserverStub);

// jsdom has no matchMedia; the pill reads the OS reduced-motion preference.
// Each test can flip `reducedMotion` before mounting.
let reducedMotion = false;
vi.stubGlobal("matchMedia", (media: string) => ({
  media,
  matches: /reduced-motion/.test(media) ? reducedMotion : false,
  addEventListener() {},
  removeEventListener() {},
}));

type PillPhase =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs: number; command: boolean }
  | { phase: "warming" }
  | { phase: "processing"; message: string };

function setPhase(phase: PillPhase): void {
  void emitTauriEvent<PillPhase>("pill-state", phase);
}

const STYLES = ["default", "classic", "levelRail", "well", "dotGrid"] as const;

async function renderPill(pillStyle: string, extra: Record<string, unknown> = {}): Promise<void> {
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

function root(): HTMLElement {
  return document.getElementById("root")!;
}

// Splits a box-shadow list on the commas between layers, not inside rgba().
function shadowLayers(value: string): string[] {
  return value
    .split(/,(?![^(]*\))/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function outerShadows(): string[] {
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

const AMBER_INSET = /inset\s+0(px)?\s+0(px)?\s+0(px)?\s+2px\s+rgba\(245,\s*197,\s*24/;

afterEach(() => {
  cleanup();
  resetTauriMocks();
  reducedMotion = false;
});

describe("H2: nothing paints outside the pill box", () => {
  for (const style of STYLES) {
    it(`${style}: a command take draws an inset amber ring and no outer shadow`, async () => {
      await renderPill(style);
      setPhase({ phase: "recording", startedAtMs: Date.now(), command: true });
      await waitFor(() => {
        const ringed = [...root().querySelectorAll<HTMLElement>("*")].filter((el) =>
          AMBER_INSET.test(el.style.boxShadow),
        );
        expect(ringed.length).toBe(1);
      });
      expect(outerShadows()).toEqual([]);
    });
  }

  for (const style of ["default", "classic", "levelRail"]) {
    it(`${style}: a command take shows a visible CMD cue beside the clock`, async () => {
      await renderPill(style);
      setPhase({ phase: "recording", startedAtMs: Date.now(), command: true });
      await waitFor(() => {
        expect(screen.getByTitle("Command take").textContent).toBe("CMD");
      });
    });

    it(`${style}: a dictation take has no CMD cue`, async () => {
      await renderPill(style);
      setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
      await waitFor(() => {
        expect(root().textContent).toContain("0:0");
      });
      expect(screen.queryByTitle("Command take")).toBeNull();
    });
  }

  it("busy and result surfaces carry no outer shadow either", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await waitFor(() => expect(screen.getByText("Transcribing…")).toBeTruthy());
    expect(outerShadows()).toEqual([]);

    await emitTauriEvent("polish-diff", { raw: "um hello there", polished: "Hello there.", ms: 900 });
    await waitFor(() => expect(root().textContent).toContain("change"));
    expect(outerShadows()).toEqual([]);
  });
});
