// claim: ui-behavior
//
// The glow pill (ui/src/pillGlow.tsx), driven through the real pill entry and
// the backend events it listens for. jsdom has no layout engine, so the test
// supplies one: a glow frame's box is the larger of its min-size floor and
// its inner shell's width, which is exactly the CSS the frame uses. The
// ResizeObserver is a stub the test fires by hand, standing in for the
// browser noticing that box change.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
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

const observers = new Set<() => void>();
class ResizeObserverStub {
  private readonly cb: () => void;
  constructor(cb: () => void) {
    this.cb = () => cb();
  }
  observe(): void {
    observers.add(this.cb);
  }
  unobserve(): void {}
  disconnect(): void {
    observers.delete(this.cb);
  }
}
vi.stubGlobal("ResizeObserver", ResizeObserverStub);

function fireResize(): void {
  act(() => {
    for (const cb of [...observers]) cb();
  });
}

function rectOf(w: number, h: number): DOMRect {
  return { x: 0, y: 0, top: 0, left: 0, right: w, bottom: h, width: w, height: h, toJSON: () => ({}) } as DOMRect;
}

// Layout stand-in for the glow frame (see the header).
function glowRect(el: HTMLElement): DOMRect {
  const frame = el.matches("[data-glow-frame]") ? el : el.querySelector<HTMLElement>("[data-glow-frame]");
  if (!frame) return rectOf(0, 0);
  const shell = frame.querySelector<HTMLElement>("[data-glow-shell]");
  const w = Math.max(parseFloat(frame.style.minWidth) || 0, shell ? parseFloat(shell.style.width) || 0 : 0);
  const h = Math.max(parseFloat(frame.style.minHeight) || 0, shell ? parseFloat(shell.style.height) || 0 : 0);
  return rectOf(w, h);
}

type Phase =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs: number; command: boolean }
  | { phase: "warming" }
  | { phase: "processing"; message: string };

async function setPhase(phase: Phase): Promise<void> {
  await act(async () => {
    await emitTauriEvent<Phase>("pill-state", phase);
  });
}

async function emit<T>(event: string, payload: T): Promise<void> {
  await act(async () => {
    await emitTauriEvent<T>(event, payload);
  });
}

function advance(ms: number): void {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

function sizeReports(): Array<{ width: number; height: number }> {
  return invokeCalls()
    .filter((c) => c.cmd === "set_pill_size")
    .map((c) => c.args as { width: number; height: number });
}

function frame(): HTMLElement {
  const el = document.querySelector<HTMLElement>("[data-glow-frame]");
  if (!el) throw new Error("no glow frame");
  return el;
}

function liveText(): string {
  return frame().querySelector('[role="status"]')?.textContent ?? "";
}

let reducedMotion = false;

async function renderPill(pillStyle = "glow"): Promise<void> {
  mockInvokeResponses({
    get_settings: { recordingMode: "hold", language: "en", inputDevice: "", pillStyle },
    list_input_devices: [],
  });
  mockListenCapture();
  document.body.innerHTML = '<div id="root"></div>';
  vi.resetModules();
  await import("../pillEntry");
  await waitFor(() => {
    expect(listenHandlersFor("pill-state")).toHaveLength(1);
    expect(invokeCalls().map((c) => c.cmd)).toContain("list_input_devices");
  });
  // Let the settings load land before driving phases.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
}

async function startRecording(command = false): Promise<void> {
  await setPhase({ phase: "recording", startedAtMs: Date.now(), command });
}

beforeEach(() => {
  observers.clear();
  reducedMotion = false;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return glowRect(this);
  });
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: (query: string) => ({
      matches: reducedMotion && query.includes("prefers-reduced-motion"),
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  cleanup();
  resetTauriMocks();
  delete (window as { matchMedia?: unknown }).matchMedia;
});

describe("glow pill: recording capsule", () => {
  it("renders 11 aria-hidden bars and reports 108x36 once on entry", async () => {
    await renderPill();
    const before = sizeReports().length;
    await startRecording();

    const bars = frame().querySelectorAll("[data-glow-bar]");
    expect(bars).toHaveLength(11);
    for (const bar of bars) expect(bar.closest('[aria-hidden="true"]')).not.toBeNull();
    expect(sizeReports().slice(before)).toEqual([{ width: 108, height: 36 }]);

    // The staged entrance widens the inner capsule, never the frame.
    advance(1000);
    fireResize();
    expect(sizeReports().slice(before)).toEqual([{ width: 108, height: 36 }]);
    expect(frame().querySelector<HTMLElement>("[data-glow-shell]")!.style.width).toBe("108px");
  });

  it("hover swaps the lane for Cancel without a resize", async () => {
    await renderPill();
    await startRecording();
    advance(1000);
    const before = sizeReports().length;

    fireEvent.mouseEnter(frame().parentElement!);
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
    expect(frame().querySelector<HTMLElement>("[data-glow-lane-wrap]")!.style.opacity).toBe("0");
    fireResize();
    expect(sizeReports().slice(before)).toEqual([]);
  });

  it("dims the lane until the first level of the take arrives", async () => {
    await renderPill();
    await startRecording();
    advance(1000);
    const lane = () => frame().querySelector<HTMLElement>("[data-glow-lane-wrap]")!;
    expect(lane().style.opacity).toBe("0.45");
    await emit<number>("pill-level", 0.4);
    expect(lane().style.opacity).toBe("1");
  });

  it("command take: the ring is inset and nothing under the frame casts an outer shadow", async () => {
    await renderPill();
    await startRecording(true);
    advance(1000);
    const shell = frame().querySelector<HTMLElement>("[data-glow-shell]")!;
    expect(shell.style.boxShadow).toContain("inset 0 0 0 2px");

    const wrapper = frame().parentElement!;
    const nodes = [wrapper, ...wrapper.querySelectorAll<HTMLElement>("*")];
    for (const node of nodes) {
      const shadow = node.style?.boxShadow ?? "";
      if (!shadow) continue;
      for (const part of shadow.split(/,(?![^(]*\))/)) {
        expect(part.trim().startsWith("inset")).toBe(true);
      }
    }
    expect(screen.getByRole("button", { name: "Stop recording command" })).toBeTruthy();
  });
});

describe("glow pill: morphs and holds", () => {
  it("recording to working: no smaller report until transitionend, then one 36x36", async () => {
    await renderPill();
    await startRecording();
    advance(1000);
    const before = sizeReports().length;

    await setPhase({ phase: "processing", message: "Transcribing…" });
    fireResize();
    advance(200);
    fireResize();
    expect(sizeReports().slice(before)).toEqual([]);

    const shell = frame().querySelector<HTMLElement>("[data-glow-shell]")!;
    expect(shell.style.width).toBe("36px");
    act(() => {
      shell.dispatchEvent(Object.assign(new Event("transitionend", { bubbles: true }), { propertyName: "width" }));
    });
    fireResize();
    expect(sizeReports().slice(before)).toEqual([{ width: 36, height: 36 }]);
    advance(1000);
    fireResize();
    expect(sizeReports().slice(before)).toEqual([{ width: 36, height: 36 }]);
  });

  it("recording to working: the 360ms fallback releases the lock when no transitionend fires", async () => {
    await renderPill();
    await startRecording();
    advance(1000);
    const before = sizeReports().length;

    await setPhase({ phase: "processing", message: "Transcribing…" });
    advance(359);
    fireResize();
    expect(sizeReports().slice(before)).toEqual([]);
    advance(1);
    fireResize();
    expect(sizeReports().slice(before)).toEqual([{ width: 36, height: 36 }]);
  });

  it("a note 100ms after the working circle appeared waits until 320ms", async () => {
    await renderPill();
    await startRecording();
    await setPhase({ phase: "processing", message: "Transcribing…" });
    expect(frame().querySelector("[data-glow-working]")).not.toBeNull();

    advance(100);
    await setPhase({ phase: "processing", message: "Pasted" });
    expect(frame().querySelector("[data-glow-note]")).toBeNull();
    expect(liveText()).toBe("Transcribing");

    advance(210);
    expect(frame().querySelector("[data-glow-note]")).toBeNull();
    advance(20);
    expect(frame().querySelector("[data-glow-note]")).not.toBeNull();
    expect(liveText()).toBe("Pasted");
  });
});

describe("glow pill: notes", () => {
  it("a 200-character error keeps the full text in title, caps at 480px, and offers Settings", async () => {
    await renderPill();
    await startRecording();
    const message = (
      "Couldn't load the speech model (missing file). Open Settings to pick another model " +
      "or download it again; the file on disk is missing or was moved while Teletype was running"
    ).padEnd(200, " and still missing");
    expect(message).toHaveLength(200);
    await setPhase({ phase: "processing", message });

    const note = frame().querySelector<HTMLElement>("[data-glow-note]")!;
    expect(note).not.toBeNull();
    expect(note.dataset.glowNote).toBe("error");
    expect(getComputedStyle(note).maxWidth).toBe("480px");
    const text = screen.getByRole("alert");
    expect(text.getAttribute("title")).toBe(message);
    expect(text.textContent).toBe(message);
    expect(screen.getByRole("button", { name: "Open Settings" })).toBeTruthy();
    // The polite region steps aside for the assertive alert.
    expect(liveText()).toBe("");
  });

  it("names every phase in the live region", async () => {
    await renderPill();
    await startRecording();
    expect(liveText()).toBe("Recording");
    expect(screen.getByRole("button", { name: "Stop recording" })).toBeTruthy();

    await setPhase({ phase: "processing", message: "Transcribing…" });
    expect(liveText()).toBe("Transcribing");

    advance(400);
    await setPhase({ phase: "processing", message: "Too short" });
    expect(liveText()).toBe("Too short");
    expect(frame().querySelector<HTMLElement>("[data-glow-note]")!.dataset.glowNote).toBe("warn");

    await setPhase({ phase: "recording", startedAtMs: Date.now() + 5, command: true });
    expect(liveText()).toBe("Recording a command");

    await setPhase({ phase: "warming" });
    expect(liveText()).toBe("Getting model ready");
    expect(frame().getAttribute("title")).toBe("Getting model ready. Press Esc to cancel.");
  });
});

describe("glow pill: reduced motion", () => {
  it("shows the lane at once, stops the ring, and keeps washes static", async () => {
    reducedMotion = true;
    await renderPill();
    await startRecording();
    const shell = frame().querySelector<HTMLElement>("[data-glow-shell]")!;
    expect(shell.style.width).toBe("108px");
    expect(frame().querySelector<HTMLElement>("[data-glow-lane-wrap]")!.style.opacity).toBe("0.45");

    await setPhase({ phase: "processing", message: "Transcribing…" });
    const ring = frame().querySelector<HTMLElement>("[data-glow-ring]")!;
    expect(ring.style.animation).toBe("");
    expect(ring.style.opacity).toBe("0.8");

    advance(400);
    await setPhase({ phase: "processing", message: "Pasted" });
    const wash = frame().querySelector<HTMLElement>("[data-glow-wash]")!;
    expect(wash.style.animation).toBe("");
    expect(wash.style.opacity).toBe("0.8");
  });
});

describe("glow pill: review fixes", () => {
  it("a level event does not force a layout of the frame", async () => {
    await renderPill();
    await startRecording();
    advance(1000);
    // Settle every layout effect the entrance staged, so the baseline is the
    // resting state: under load a still-scheduled effect would read the frame
    // after the baseline and look like a level-forced read.
    advance(200);
    await emit<number>("pill-level", 0.3);
    const frameReads = () =>
      (HTMLElement.prototype.getBoundingClientRect as unknown as { mock: { contexts: unknown[] } }).mock.contexts.filter(
        (el) => el instanceof HTMLElement && el.matches("[data-glow-frame]"),
      ).length;
    const before = frameReads();
    // Positive control: the frame must have been measured at least once before
    // the level events, otherwise "no extra reads" is true of a frame nobody
    // ever laid out and the test proves nothing.
    expect(before).toBeGreaterThan(0);
    for (let i = 0; i < 5; i++) await emit<number>("pill-level", 0.2 + i / 10);
    expect(frameReads()).toBe(before);
  });

  it("the working glow is a 36x36 square centred in the morphing shell", async () => {
    await renderPill();
    await startRecording();
    advance(1000);
    await setPhase({ phase: "processing", message: "Transcribing…" });
    const layers = frame().querySelector<HTMLElement>("[data-glow-working]")!;
    expect(layers.style.inset).toBe("");
    expect([layers.style.width, layers.style.height, layers.style.left, layers.style.marginLeft]).toEqual([
      "36px",
      "36px",
      "50%",
      "-18px",
    ]);
  });

  it("speaks the clamped skip line and names the keyboard path on working and note frames", async () => {
    await renderPill();
    await startRecording();
    await setPhase({ phase: "processing", message: "Transcribing…" });
    expect(frame().getAttribute("title")).toBe("Transcribing. Press Esc to cancel.");
    advance(400);
    await emit("pill-skip", { message: "Polish skipped: no model loaded. Using raw text" });
    await setPhase({ phase: "processing", message: "Inserting…" });
    expect(frame().querySelector<HTMLElement>("[data-glow-note]")!.dataset.glowNote).toBe("warn");
    expect(liveText()).toBe("Inserting. Polish skipped: no model loaded. Using raw text");
    expect(frame().getAttribute("title")).toBe("Press your hotkey to dictate again.");
  });

  it("error text stays primary (the glyph and wash carry the tone) and the action sits 10px from it", async () => {
    await renderPill();
    await startRecording();
    await setPhase({ phase: "processing", message: "Couldn't load the speech model (re-download it in Settings)" });
    expect(screen.getByRole("alert").style.color).toBe("rgba(255, 255, 255, 0.92)");
    const action = screen.getByRole("button", { name: "Open Settings" });
    expect(action.style.margin).toBe("-4px 0px -4px 2px");
    expect(frame().querySelector<HTMLElement>("[data-glow-note]")!.style.gap).toBe("8px");
  });

  it("more contrast makes the surface opaque instead of doubling the wash", async () => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: (query: string) => ({
        matches: query.includes("prefers-contrast"),
        media: query,
        addEventListener: () => {},
        removeEventListener: () => {},
      }),
    });
    await renderPill();
    await startRecording();
    await setPhase({ phase: "processing", message: "Transcription failed" });
    const note = frame().querySelector<HTMLElement>("[data-glow-note]")!;
    expect(note.style.background).toBe("rgb(20, 20, 28)");
    expect(frame().querySelector<HTMLElement>("[data-glow-wash]")!.style.background).toBe("rgba(255, 69, 61, 0.18)");
  });

  it("reduced motion keeps the cross-fades at 120ms and exempts them from the blanket rule", async () => {
    reducedMotion = true;
    await renderPill();
    await startRecording();
    await setPhase({ phase: "processing", message: "Transcribing…" });
    const working = frame().querySelector<HTMLElement>("[data-glow-working]")!;
    expect(working.hasAttribute("data-glow-fade")).toBe(true);
    expect(working.style.animation).toContain("120ms");
    advance(400);
    await setPhase({ phase: "processing", message: "Pasted" });
    const note = frame().querySelector<HTMLElement>("[data-glow-note]")!;
    expect(note.hasAttribute("data-glow-fade")).toBe(true);
    expect(note.style.animation).toContain("120ms");
  });

  it("the app's Reduce Motion setting stills the glow like the OS preference", async () => {
    mockInvokeResponses({
      get_settings: { recordingMode: "hold", language: "en", inputDevice: "", pillStyle: "glow", reduceMotion: true },
      list_input_devices: [],
    });
    mockListenCapture();
    document.body.innerHTML = '<div id="root"></div>';
    vi.resetModules();
    await import("../pillEntry");
    await waitFor(() => expect(listenHandlersFor("pill-state")).toHaveLength(1));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    expect(document.documentElement.dataset.motion).toBe("reduced");
    await startRecording();
    // Reduced: no staged entrance, the capsule is wide at once.
    expect(frame().querySelector<HTMLElement>("[data-glow-shell]")!.style.width).toBe("108px");
    await setPhase({ phase: "processing", message: "Transcribing…" });
    expect(frame().querySelector<HTMLElement>("[data-glow-ring]")!.style.animation).toBe("");
    delete document.documentElement.dataset.motion;
  });
});

describe("pill style whitelist (M6)", () => {
  it("an unknown style falls back to default and still renders the recording pill", async () => {
    await renderPill("futureStyle");
    await startRecording();
    expect(screen.getByTitle("Recording. Click or press your hotkey to stop.")).toBeTruthy();
    expect(document.getElementById("root")!.textContent).toContain("0:00");
    expect(document.querySelector("[data-glow-frame]")).toBeNull();
  });
});
