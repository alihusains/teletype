// claim: ui-behavior
// Regression tests for the existing-pill layout defects in
// research/pills/PILL_SPEC.md section 11 (H1-H5, M1-M8, L1-L6). Each describe
// block names the defect it pins.
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

class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
vi.stubGlobal("ResizeObserver", ResizeObserverStub);

// Frame callbacks are counted, never run: the tests assert which states
// schedule per-frame work, not the smoothing itself.
let rafRequests = 0;
vi.stubGlobal("requestAnimationFrame", () => ++rafRequests);
vi.stubGlobal("cancelAnimationFrame", () => {});

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

function lastPillSize(): { width: number; height: number } | undefined {
  const calls = invokeCalls().filter((c) => c.cmd === "set_pill_size");
  return calls[calls.length - 1]?.args as { width: number; height: number } | undefined;
}

const AMBER_INSET = /inset\s+0(px)?\s+0(px)?\s+0(px)?\s+2px\s+rgba\(245,\s*197,\s*24/;

afterEach(() => {
  cleanup();
  resetTauriMocks();
  reducedMotion = false;
  rafRequests = 0;
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

  it("busy and result surfaces carry no outer shadow either (dotGrid)", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await waitFor(() => expect(screen.getByText("Transcribing…")).toBeTruthy());
    expect(outerShadows()).toEqual([]);

    await emitTauriEvent("polish-diff", { raw: "um hello there", polished: "Hello there.", ms: 900 });
    await waitFor(() => expect(root().textContent).toContain("change"));
    expect(outerShadows()).toEqual([]);
  });
});

describe("H4: error and long notes are sized to their text", () => {
  const MODEL_ERROR =
    "Couldn't load the speech model (model file is missing, re-download it in Settings)";

  for (const style of ["default", "classic", "levelRail", "well"]) {
    it(`${style}: the model-load error keeps its full text and the settings action`, async () => {
      await renderPill(style);
      setPhase({ phase: "processing", message: MODEL_ERROR });
      await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
      const alert = screen.getByRole("alert");
      expect(alert.textContent).toBe(MODEL_ERROR);
      expect(alert.getAttribute("title")).toBe(MODEL_ERROR);
      expect(screen.getByRole("button", { name: "Open Settings" })).toBeTruthy();
      const size = lastPillSize()!;
      expect(size.width).toBeGreaterThanOrEqual(340);
      expect(size.width).toBeLessThanOrEqual(480);
      // Too long for one line at the cap: two clamped lines, a taller pill.
      expect(size.height).toBe(64);
      expect(alert.style.webkitLineClamp).toBe("2");
    });
  }

  it("a short status keeps the 220x52 pill", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Pasted" });
    await waitFor(() => expect(screen.getByText("Pasted")).toBeTruthy());
    expect(lastPillSize()).toEqual({ width: 220, height: 52 });
  });
});

function litDots(): string {
  const grid = root().querySelector<HTMLElement>("[data-dot-grid]")!;
  return [...grid.children]
    .map((el, i) => ((el as HTMLElement).style.opacity === "1" ? i : -1))
    .filter((i) => i >= 0)
    .join(",");
}

describe("H5: Dot Matrix busy states animate without a per-frame loop", () => {
  it("the processing row scan moves over time and schedules no frames", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await waitFor(() => expect(root().querySelector('[data-dot-grid="thinking"]')).toBeTruthy());
    const first = litDots();
    await new Promise((r) => setTimeout(r, 300));
    expect(litDots()).not.toBe(first);
    expect(rafRequests).toBe(0);
  });

  it("the warming ring sweep moves over time", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "warming" });
    await waitFor(() => expect(root().querySelector('[data-dot-grid="connecting"]')).toBeTruthy());
    const first = litDots();
    await new Promise((r) => setTimeout(r, 300));
    expect(litDots()).not.toBe(first);
  });

  it("reduced motion shows a still, legible frame (whole middle row)", async () => {
    reducedMotion = true;
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await waitFor(() => expect(root().querySelector('[data-dot-grid="thinking"]')).toBeTruthy());
    const first = litDots();
    expect(first.split(",")).toHaveLength(25);
    await new Promise((r) => setTimeout(r, 300));
    expect(litDots()).toBe(first);
  });

  it("reduced motion: the speaking grid still tracks each level event", async () => {
    reducedMotion = true;
    await renderPill("dotGrid");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await waitFor(() => expect(screen.getByText("LISTENING")).toBeTruthy());
    for (let i = 0; i < 8; i++) await emitTauriEvent<number>("pill-level", 0.2);
    await waitFor(() => expect(root().querySelector('[data-dot-grid="speaking"]')).toBeTruthy());
    const quiet = litDots();
    for (let i = 0; i < 8; i++) await emitTauriEvent<number>("pill-level", 1);
    await waitFor(() => expect(litDots()).not.toBe(quiet));
    expect(rafRequests).toBe(0);
  });
});

const DEVICES = [
  { id: "mbp", name: "MacBook Pro Microphone", is_default: true },
  { id: "ext", name: "Shure MV7+ USB Podcast Microphone", is_default: false },
];

function hoverPill(): void {
  // The hover target is the element that carries the pill's mouse handlers:
  // the measured wrapper (ported styles) or the capsule column (default).
  const target = root().firstElementChild!.firstElementChild as HTMLElement;
  fireEvent.mouseEnter(target);
}

describe("H1: the device list renders inside the pill and never eats a click", () => {
  for (const style of ["well", "dotGrid"]) {
    it(`${style}: a long-press opens the list inline, and Cancel still works first click`, async () => {
      await renderPill(style, {
        list_input_devices: DEVICES,
        cancel_dictation: undefined,
        update_settings_patch: { recordingMode: "hold", language: "en", inputDevice: "ext", pillStyle: style },
      });
      setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
      await waitFor(() => expect(screen.getByText("MacBook")).toBeTruthy());
      hoverPill();
      const chip = screen.getByText("MacBook").closest("button")!;
      fireEvent.pointerDown(chip);
      await new Promise((r) => setTimeout(r, 550));
      fireEvent.pointerUp(chip);
      fireEvent.click(chip);

      const option = await screen.findByText("Shure MV7+ USB Podcast Microphone");
      // Inline: inside the panel, in normal flow, with no click shield.
      const panel = root().firstElementChild!.firstElementChild!;
      expect(panel.contains(option)).toBe(true);
      for (const el of root().querySelectorAll<HTMLElement>("*")) {
        expect(el.style.position).not.toBe("fixed");
        expect(el.style.position === "absolute" && el.contains(option)).toBe(false);
      }

      // The first press outside the list closes it and still reaches Cancel.
      const cancel = screen.getByRole("button", { name: "Cancel" });
      fireEvent.pointerDown(cancel);
      fireEvent.click(cancel);
      expect(screen.getByRole("button", { name: "Sure?" })).toBeTruthy();
      await waitFor(() => expect(screen.queryByText("Shure MV7+ USB Podcast Microphone")).toBeNull());
    });
  }

  it("default: the mic chip list grows the reported window by its exact height", async () => {
    await renderPill("default", { list_input_devices: DEVICES });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await waitFor(() => expect(screen.getByTitle("Recording. Click or press your hotkey to stop.")).toBeTruthy());
    hoverPill();
    const chip = await screen.findByText("MacBook");
    const collapsed = lastPillSize()!;
    fireEvent.click(chip.closest("button")!);
    await screen.findByText("Shure MV7+ USB Podcast Microphone");
    // Two 24px rows plus 8px padding, plus the 4px gap to the capsule.
    expect(collapsed).toEqual({ width: 540, height: 52 });
    expect(lastPillSize()).toEqual({ width: collapsed.width, height: collapsed.height + 4 + 56 });

    // A press anywhere outside closes it and the window shrinks back.
    fireEvent.pointerDown(document.body);
    await waitFor(() => expect(screen.queryByText("Shure MV7+ USB Podcast Microphone")).toBeNull());
    await waitFor(() => expect(lastPillSize()).toEqual(collapsed));
  });
});

describe("H3: the polish card hands the window back to Rust on dismiss", () => {
  const DIFF = { raw: "um so hello there", polished: "So, hello there.", ms: 900 };

  it("dismissing the card reports its id so Rust can hide the window", async () => {
    await renderPill("classic", { dismiss_pill_result: undefined });
    await emitTauriEvent("polish-diff", { ...DIFF, card: 7 });
    const dismiss = await screen.findByRole("button", { name: "Dismiss" });
    fireEvent.click(dismiss);
    await waitFor(() => {
      expect(invokeCalls()).toContainEqual({ cmd: "dismiss_pill_result", args: { card: 7 } });
    });
  });

  it("a take that replaces the card never sends a dismiss", async () => {
    await renderPill("classic", { dismiss_pill_result: undefined });
    await emitTauriEvent("polish-diff", { ...DIFF, card: 8 });
    await screen.findByRole("button", { name: "Dismiss" });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await waitFor(() => expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull());
    await new Promise((r) => setTimeout(r, 20));
    expect(invokeCalls().some((c) => c.cmd === "dismiss_pill_result")).toBe(false);
  });
});

describe("M1: the Dot Matrix busy capsule is capped and offers the settings action", () => {
  const MODEL_ERROR =
    "Couldn't load the speech model (model file is missing, re-download it in Settings)";

  it("an error wraps to two clamped lines inside a 480px cap, with Open Settings", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: MODEL_ERROR });
    const alert = await screen.findByRole("alert");
    expect(alert.getAttribute("title")).toBe(MODEL_ERROR);
    expect(alert.style.webkitLineClamp).toBe("2");
    expect(alert.style.whiteSpace).toBe("normal");
    const capsule = root().querySelector("[data-dot-grid]")!.parentElement!;
    expect(capsule.style.maxWidth).toBe("480px");
    expect(capsule.style.minHeight).toBe("44px");
    expect(capsule.style.height).toBe("");
    expect(screen.getByRole("button", { name: "Open Settings" })).toBeTruthy();
  });

  it("an ordinary error gets no settings action", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: "Transcription failed" });
    await screen.findByRole("alert");
    expect(screen.queryByRole("button", { name: "Open Settings" })).toBeNull();
  });

  it("the skip line keeps its full text in the title", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: "Inserting…" });
    const skip = "Polish skipped: no model loaded. Using raw text";
    await emitTauriEvent("pill-skip", { message: skip });
    expect((await screen.findByText(skip)).getAttribute("title")).toBe(skip);
  });
});

describe("M2: the hover-expanded default pill keeps its meter", () => {
  it("expands to 540px and shows a detected language as a dot, not a suffix", async () => {
    await renderPill("default", { list_input_devices: DEVICES });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await emitTauriEvent("pill-language", { code: "ru", detected: true });
    await waitFor(() => expect(screen.getByTitle("Recording. Click or press your hotkey to stop.")).toBeTruthy());
    hoverPill();
    await screen.findByText("RU");
    expect(root().textContent).not.toContain("detected");
    expect(screen.getByRole("img", { name: "detected" })).toBeTruthy();
    expect(lastPillSize()).toEqual({ width: 540, height: 52 });
    const meter = screen.getByTitle("Recording. Click or press your hotkey to stop.").nextElementSibling as HTMLElement;
    expect(meter.style.minWidth).toBe("72px");
  });
});

describe("M3: shrink transitions keep the rounded ends inside the window", () => {
  it("holds the expanded width until the capsule's width transition ends", async () => {
    await renderPill("default", { list_input_devices: DEVICES });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await waitFor(() => expect(screen.getByTitle("Recording. Click or press your hotkey to stop.")).toBeTruthy());
    hoverPill();
    await waitFor(() => expect(lastPillSize()).toEqual({ width: 540, height: 52 }));

    fireEvent.mouseLeave(root().firstElementChild!.firstElementChild!);
    await waitFor(() => expect(screen.queryByText("Hold")).toBeNull());
    // The capsule is animating 540 -> 250; the window must not shrink yet.
    expect(lastPillSize()).toEqual({ width: 540, height: 52 });

    const capsule = screen.getByTitle("Recording. Click or press your hotkey to stop.").closest(
      "div[style*='transition']",
    ) as HTMLElement;
    fireEvent.transitionEnd(capsule, { propertyName: "width" });
    expect(lastPillSize()).toEqual({ width: 250, height: 52 });
  });

  it("falls back to reporting the smaller size after 500ms", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Couldn't load the speech model (re-download it in Settings)" });
    await waitFor(() => expect(lastPillSize()?.width).toBeGreaterThan(220));
    const wide = lastPillSize()!;
    setPhase({ phase: "processing", message: "Pasted" });
    await screen.findByText("Pasted");
    expect(lastPillSize()).toEqual(wide);
    await new Promise((r) => setTimeout(r, 550));
    expect(lastPillSize()).toEqual({ width: 220, height: 52 });
  });
});

describe("M4: Level Rail text never sits under the bars", () => {
  const LONG =
    "So the plan for the quarterly review is to walk through the onboarding funnel first, then the retention numbers.";

  it("fixes the meter beside the text and shows the newest words", async () => {
    await renderPill("levelRail");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await emitTauriEvent("interim-transcript", LONG);
    const slot = await screen.findByTitle(LONG);
    expect(slot.style.flex).toBe("1 1 0px");
    expect(slot.style.minWidth).toBe("0px");
    expect(slot.style.maxWidth).toBe("");
    expect(slot.style.justifyContent).toBe("flex-end");
    expect(slot.textContent!.startsWith("…")).toBe(true);
    expect(slot.textContent!.endsWith("the retention numbers.")).toBe(true);
    const meter = root().querySelector<HTMLElement>("[data-rail-meter]")!;
    expect(meter.style.flex).toBe("0 0 auto");
    expect(meter.children).toHaveLength(16);
  });

  it("hides the text while hovered so Cancel fits", async () => {
    await renderPill("levelRail");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await emitTauriEvent("interim-transcript", LONG);
    await screen.findByTitle(LONG);
    hoverPill();
    await screen.findByRole("button", { name: "Cancel" });
    expect(screen.queryByTitle(LONG)).toBeNull();
  });
});

describe("M5: hover and new lines never jump the panel styles", () => {
  for (const style of ["well", "dotGrid"]) {
    it(`${style}: hover swaps the badge for Cancel in place, with no new row`, async () => {
      await renderPill(style, { list_input_devices: DEVICES });
      setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
      const badge = await screen.findByText("LISTENING");
      const nodesBefore = root().querySelectorAll("*").length;
      // Cancel is already laid out (so the slot keeps its width), just hidden.
      const cancelSlot = screen.getByText("Cancel").closest("button")!.parentElement as HTMLElement;
      expect(cancelSlot.style.visibility).toBe("hidden");
      expect(cancelSlot.style.pointerEvents).toBe("none");
      expect(cancelSlot.parentElement).toBe(badge.parentElement);

      hoverPill();
      await waitFor(() => expect(cancelSlot.style.visibility).toBe("visible"));
      expect((badge as HTMLElement).style.visibility).toBe("hidden");
      expect(root().querySelectorAll("*").length).toBe(nodesBefore);
    });
  }

  it("aligns the content to the anchored edges of the slot", async () => {
    await renderPill("well", {
      get_settings: { recordingMode: "hold", language: "en", inputDevice: "", pillStyle: "well", pillPosition: "topLeft" },
    });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await screen.findByText("LISTENING");
    const top = root().firstElementChild as HTMLElement;
    expect(top.style.alignItems).toBe("flex-start");
    expect(top.style.justifyContent).toBe("flex-start");
  });

  it("defaults to the bottom-centre slot", async () => {
    await renderPill("well");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await screen.findByText("LISTENING");
    const top = root().firstElementChild as HTMLElement;
    expect(top.style.alignItems).toBe("flex-end");
    expect(top.style.justifyContent).toBe("center");
  });
});

describe("M7: skip messages are stacked, not cut mid-sentence", () => {
  const SKIP = "Polish skipped: no model loaded. Using raw text";

  it("default: status and skip sit on separate lines in a 480x64 pill", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Inserting…" });
    await emitTauriEvent("pill-skip", { message: SKIP });
    const skip = await screen.findByText(SKIP);
    const status = screen.getByText("Inserting…");
    expect(skip.getAttribute("title")).toBe(SKIP);
    expect(status.getAttribute("title")).toBe("Inserting…");
    expect(skip.parentElement).toBe(status.parentElement);
    expect((skip.parentElement as HTMLElement).style.flexDirection).toBe("column");
    await waitFor(() => expect(lastPillSize()).toEqual({ width: 480, height: 64 }));
  });

  it("keeps the taller skip size when the take also had a mic failure", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "No audio captured" });
    await emitTauriEvent("mic-failure", undefined);
    await emitTauriEvent("pill-skip", { message: SKIP });
    await screen.findByText(SKIP);
    await waitFor(() => expect(lastPillSize()).toEqual({ width: 480, height: 64 }));
    expect(screen.getByRole("button", { name: "Open Settings" })).toBeTruthy();
  });
});

describe("M8: the polish card clamps to three whole lines and counts the rest", () => {
  const RAW =
    "um so the plan for the uh quarterly review is to walk through the onboarding funnel first then the retention numbers for the new cohort and finally the open questions about pricing in europe";
  const POLISHED =
    "So the plan for the quarterly review is to walk through the onboarding funnel first, then the retention numbers for the new cohort, and finally the open questions about pricing in Europe.";

  it("uses a three-line clamp instead of a 60px max height", async () => {
    await renderPill("classic");
    await emitTauriEvent("polish-diff", { raw: RAW, polished: POLISHED, ms: 1240 });
    await screen.findByRole("button", { name: "Dismiss" });
    const body = root().querySelector<HTMLElement>("[data-change]")!.parentElement!;
    expect(body.style.webkitLineClamp).toBe("3");
    expect(body.style.maxHeight).toBe("");
    expect(body.style.lineHeight).toBe("19.5px");
  });

  it("names the changes that start below the clamp", async () => {
    // Lay each change out 15px lower than the last, so the later ones fall
    // past the 58.5px clamp of a body whose top is at 0.
    const spy = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ) {
      const marks = [...document.querySelectorAll("[data-change]")];
      const i = marks.indexOf(this);
      const top = i < 0 ? 0 : i * 15;
      return { top, left: 0, right: 460, bottom: top + 19.5, width: 460, height: 19.5, x: 0, y: top, toJSON() {} } as DOMRect;
    });
    try {
      await renderPill("classic");
      await emitTauriEvent("polish-diff", { raw: RAW, polished: POLISHED, ms: 1240 });
      await screen.findByRole("button", { name: "Dismiss" });
      const marks = root().querySelectorAll("[data-change]").length;
      const below = [...Array(marks).keys()].filter((i) => i * 15 >= 3 * 19.5 - 1).length;
      expect(below).toBeGreaterThan(0);
      await waitFor(() => expect(root().textContent).toContain(`+${below} more change`));
    } finally {
      spy.mockRestore();
    }
  });
});
