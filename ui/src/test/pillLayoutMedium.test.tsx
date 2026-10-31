// claim: ui-behavior
// Regression tests for the medium-severity defects (M1-M8; M6 ships with the glow style) in the existing pill styles,
// research/pills/PILL_SPEC.md section 11. Each describe block names the
// defect it pins. Shared mounting and stubs live in ./pillHarness.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { emitTauriEvent, resetTauriMocks } from "./tauriMock";
import {
  DEVICES,
  hoverPill,
  lastPillSize,
  renderPill,
  resetPillHarness,
  root,
  setPhase,
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

  it("reports the next take at once after a wide note goes idle in a hidden window", async () => {
    // always_show off: idle hides the window, and Rust shows the next take
    // only once its size arrives. A deferred idle shrink used to hold that
    // report back (250x52 fits inside the wide note, so it deduped).
    await renderPill("default");
    setPhase({ phase: "processing", message: "Couldn't load the speech model (re-download it in Settings)" });
    await waitFor(() => expect(lastPillSize()?.width).toBeGreaterThan(250));
    // Well inside the 500ms shrink fallback, so only an immediate report passes.
    setPhase({ phase: "idle" });
    await waitFor(() => expect(lastPillSize()).toEqual({ width: 58, height: 24 }), { timeout: 200 });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await waitFor(() => expect(lastPillSize()).toEqual({ width: 250, height: 52 }), { timeout: 200 });
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
