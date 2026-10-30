// claim: ui-behavior
// T02: pill overlay UI fixes — error visual, reduced motion, size-report
// dedupe, aria-live, esc hint.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import {
  emitTauriEvent,
  invokeCalls,
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

// jsdom has no layout engine: the pill's size reporter and the dot grid's
// rAF loop need stand-ins. The rAF stub records every frame callback so the
// reduced-motion test can assert the loop never started without waiting.
const rafCbs = new Set<() => void>();
function rafStub(cb: FrameRequestCallback): number {
  rafCbs.add(() => cb(performance.now()));
  return rafCbs.size;
}
function cafStub(id: number): void {
  rafCbs.delete([...rafCbs][id - 1]);
}
vi.stubGlobal("requestAnimationFrame", rafStub);
vi.stubGlobal("cancelAnimationFrame", cafStub);
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
vi.stubGlobal("ResizeObserver", ResizeObserverStub);

function countRaf(): number {
  return rafCbs.size;
}

// jsdom has no matchMedia; the pill reads the OS reduced-motion preference.
// The default is "no preference"; each test can flip `reducedMotion`.
let reducedMotion = false;
interface Mql {
  media: string;
  matches: boolean;
  addEventListener: (t: string, cb: (e: { matches: boolean }) => void) => void;
  removeEventListener: (t: string, cb: (e: { matches: boolean }) => void) => void;
  _cb: ((e: { matches: boolean }) => void) | null;
}
function makeMql(): Mql {
  return {
    media: "prefers-reduced-motion: reduce",
    matches: reducedMotion,
    addEventListener(type: string, cb: (e: { matches: boolean }) => void) {
      if (type === "change") this._cb = cb;
    },
    removeEventListener() {},
    _cb: null,
  };
}
let mql: Mql;
vi.stubGlobal("matchMedia", () => {
  mql = makeMql();
  return mql;
});
function setReducedMotion(v: boolean): void {
  reducedMotion = v;
  if (mql) mql._cb?.({ matches: v });
}

// The pill entry point renders itself into #root on import (createRoot in
// pillEntry.tsx), so the test provides a #root container before importing the
// module, exactly like pillStates.test.tsx.
type PillPhase =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs: number; command: boolean }
  | { phase: "warming" }
  | { phase: "processing"; message: string };

function setPhase(phase: PillPhase): void {
  void emitTauriEvent<PillPhase>("pill-state", phase);
}

async function renderPill(pillStyle = "default"): Promise<void> {
  mockInvokeResponses({
    get_settings: {
      recordingMode: "hold",
      language: "en",
      inputDevice: "",
      pillStyle,
    },
    list_input_devices: [],
    set_pill_size: undefined,
  });
  mockListenCapture();
  document.body.innerHTML = '<div id="root"></div>';
  vi.resetModules();
  await import("../pillEntry");
  await waitFor(() => {
    expect(listenHandlersFor("pill-state")).toHaveLength(1);
  });
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
  rafCbs.clear();
  reducedMotion = false;
});

describe("pill error visual", () => {
  it("renders a distinct error body, not the processing body", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Transcription failed" });

    await waitFor(() => {
      expect(screen.getByText("Transcription failed")).toBeTruthy();
    });
    // The error body is its own container (role="alert"), not the
    // processing body (role="status").
    expect(document.querySelector('[role="alert"]')).toBeTruthy();
    expect(document.querySelector('[role="alert"]')!.textContent).toContain(
      "Transcription failed",
    );
  });

  it("offers a recovery action for a settings-fixable error", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Model not found (error). Check Settings." });

    await waitFor(() => {
      expect(screen.getByText("Model not found (error). Check Settings.")).toBeTruthy();
    });
    const action = await screen.findByRole("button", { name: "Open Settings" });
    fireEvent.click(action);
    await waitFor(() => {
      expect(invokeCalls()).toContainEqual({
        cmd: "open_main_window",
        args: { route: "settings" },
      });
    });
  });

  it("does not offer a recovery action for an ordinary error", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Transcription failed" });

    await waitFor(() => {
      expect(screen.getByText("Transcription failed")).toBeTruthy();
    });
    expect(screen.queryByRole("button", { name: "Open Settings" })).toBeNull();
  });
});

describe("pill reduced motion", () => {
  it("does not start the dot grid rAF loop when the OS prefers reduced motion", async () => {
    setReducedMotion(true);
    await renderPill("dotGrid");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });

    await waitFor(() => {
      expect(screen.getByText("LISTENING")).toBeTruthy();
    });
    // The speaking hysteresis effect and the idle-fade effect would each
    // schedule a frame in normal motion; with reduced motion the grid is a
    // static frame, so no frame callback may exist.
    expect(countRaf()).toBe(0);
  });

  it("starts the dot grid rAF loop when motion is not reduced", async () => {
    setReducedMotion(false);
    await renderPill("dotGrid");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });

    await waitFor(() => {
      expect(screen.getByText("LISTENING")).toBeTruthy();
    });
    expect(countRaf()).toBeGreaterThan(0);
  });
});

describe("pill size reporting", () => {
  it("reports the processing size once, even as stream tokens arrive", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Transforming…" });

    await waitFor(() => {
      expect(screen.getByText("Transforming…")).toBeTruthy();
    });
    const sizeCalls = () =>
      invokeCalls().filter((c) => c.cmd === "set_pill_size");
    expect(sizeCalls().length).toBeGreaterThan(0);
    const before = sizeCalls().length;

    // Each polish token re-renders the pill; the layout effect must not
    // re-report a size that did not change.
    await emitTauriEvent<{ seq: number; tok: string }>("transform-token", {
      seq: 1,
      tok: "Hello",
    });
    await emitTauriEvent<{ seq: number; tok: string }>("transform-token", {
      seq: 2,
      tok: " world",
    });
    // Give effects a beat to (wrongly) re-run.
    await new Promise((r) => setTimeout(r, 50));

    expect(sizeCalls().length).toBe(before);
  });
});

describe("pill status accessibility", () => {
  it("marks the processing message container as a polite live region", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Transcribing…" });

    await waitFor(() => {
      expect(screen.getByText("Transcribing…")).toBeTruthy();
    });
    const container = screen.getByText("Transcribing…").closest('[aria-live="polite"]');
    expect(container).toBeTruthy();
  });
});

describe("pill warming esc hint", () => {
  it("shows the esc hint (a global cancel hotkey is armed while a take is alive)", async () => {
    await renderPill("default");
    setPhase({ phase: "warming" });

    await waitFor(() => {
      expect(screen.getByText("Getting model ready…")).toBeTruthy();
    });
    expect(screen.getByText("esc")).toBeTruthy();
  });
});
