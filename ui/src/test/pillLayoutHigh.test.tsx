// claim: ui-behavior
// Regression tests for the high-severity defects (H1-H5) in the existing pill styles,
// research/pills/PILL_SPEC.md section 11. Each describe block names the
// defect it pins. Shared mounting and stubs live in ./pillHarness.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { emitTauriEvent, invokeCalls, resetTauriMocks } from "./tauriMock";
import {
  AMBER_INSET,
  DEVICES,
  hoverPill,
  lastPillSize,
  litDots,
  outerShadows,
  rafRequests,
  renderPill,
  resetPillHarness,
  root,
  setPhase,
  setReducedMotion,
  STYLES,
} from "./pillHarness";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

afterEach(() => {
  cleanup();
  resetTauriMocks();
  resetPillHarness();
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

describe("H5: Dot Matrix busy states animate without a per-frame loop", () => {
  it("the processing row scan moves over time and schedules no frames", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await waitFor(() => expect(root().querySelector('[data-dot-grid="thinking"]')).toBeTruthy());
    const first = litDots();
    await new Promise((r) => setTimeout(r, 300));
    expect(litDots()).not.toBe(first);
    expect(rafRequests()).toBe(0);
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
    setReducedMotion(true);
    await renderPill("dotGrid");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await waitFor(() => expect(root().querySelector('[data-dot-grid="thinking"]')).toBeTruthy());
    const first = litDots();
    expect(first.split(",")).toHaveLength(25);
    await new Promise((r) => setTimeout(r, 300));
    expect(litDots()).toBe(first);
  });

  it("reduced motion: the speaking grid still tracks each level event", async () => {
    setReducedMotion(true);
    await renderPill("dotGrid");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await waitFor(() => expect(screen.getByText("LISTENING")).toBeTruthy());
    for (let i = 0; i < 8; i++) await emitTauriEvent<number>("pill-level", 0.2);
    await waitFor(() => expect(root().querySelector('[data-dot-grid="speaking"]')).toBeTruthy());
    const quiet = litDots();
    for (let i = 0; i < 8; i++) await emitTauriEvent<number>("pill-level", 1);
    await waitFor(() => expect(litDots()).not.toBe(quiet));
    expect(rafRequests()).toBe(0);
  });
});

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

describe("H2 follow-up: the hover area keeps the pill's rounded corners", () => {
  for (const [style, radius] of [
    ["classic", "22px"],
    ["levelRail", "22px"],
    ["well", "16px"],
    ["dotGrid", "16px"],
  ] as const) {
    it(`${style}: the hover wrapper is ${radius} rounded`, async () => {
      await renderPill(style);
      setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
      await waitFor(() => expect(root().textContent).toContain("0:0"));
      const wrapper = root().firstElementChild!.firstElementChild as HTMLElement;
      expect(wrapper.style.borderRadius).toBe(radius);
    });
  }
});
