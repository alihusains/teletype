// claim: ui-behavior
// Regression tests for the low-severity defects (L1-L6) in the existing pill styles,
// research/pills/PILL_SPEC.md section 11. Each describe block names the
// defect it pins. Shared mounting and stubs live in ./pillHarness.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { emitTauriEvent, invokeCalls, resetTauriMocks } from "./tauriMock";
import {
  DEVICES,
  hoverPill,
  renderPill,
  resetPillHarness,
  root,
  setPhase,
  setReducedMotion,
  textAlphas,
} from "./pillHarness";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  resetTauriMocks();
  resetPillHarness();
});

describe("L1: pill hit targets are at least 24px", () => {
  it("Cancel and the mic chip are 24px tall", async () => {
    await renderPill("well", { list_input_devices: DEVICES });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await screen.findByText("LISTENING");
    hoverPill();
    const cancel = await screen.findByRole("button", { name: "Cancel" });
    expect(cancel.style.minHeight).toBe("24px");
    expect(cancel.style.padding).toBe("5px 10px");
    const chip = screen.getByText("MacBook").closest("button")!;
    expect(chip.style.minHeight).toBe("24px");
    expect(chip.style.padding).toBe("5px 10px");
  });

  it("the card's dismiss is 24x24 and Configure Polish is at least 24px tall", async () => {
    await renderPill("classic");
    await emitTauriEvent("polish-diff", { raw: "um hello there", polished: "Hello there.", ms: 900 });
    const dismiss = await screen.findByRole("button", { name: "Dismiss" });
    expect([dismiss.style.width, dismiss.style.height]).toEqual(["24px", "24px"]);
    // 4px 6px padding with normal line height rendered 22px tall in
    // Chromium; the rendered height is checked in the pill audit harness.
    const configure = screen.getByRole("button", { name: "Configure Polish" });
    expect(configure.style.minHeight).toBe("24px");
    expect(configure.style.lineHeight).toBe("14px");
  });

  it("the clamped polish body keeps the full polished text in its title", async () => {
    await renderPill("classic");
    const polished = "First change. Second change. Third change. Fourth change. Fifth change. Sixth change.";
    await emitTauriEvent("polish-diff", { raw: "first second third fourth fifth sixth", polished, ms: 900 });
    const body = await screen.findByRole("button", { name: /click to open History/ });
    expect(body.getAttribute("title")).toBe(polished);
  });
});

describe("L2: secondary text stays legible and nothing blurs a transparent window", () => {
  it("warming: the esc hint is at least 0.6 alpha", async () => {
    await renderPill("default");
    setPhase({ phase: "warming" });
    await screen.findByText("esc");
    for (const t of textAlphas()) expect(t.alpha, t.text).toBeGreaterThanOrEqual(0.6);
  });

  it("dotGrid warming: same rule", async () => {
    await renderPill("dotGrid");
    setPhase({ phase: "warming" });
    await screen.findByText("esc");
    for (const t of textAlphas()) expect(t.alpha, t.text).toBeGreaterThanOrEqual(0.6);
  });

  it("polish card: timing and footer at least 0.6, deleted words at least 0.5", async () => {
    await renderPill("classic");
    await emitTauriEvent("polish-diff", { raw: "um so hello there", polished: "So, hello there.", ms: 1200 });
    await screen.findByRole("button", { name: "Dismiss" });
    const alphas = textAlphas();
    expect(alphas.some((t) => t.struck)).toBe(true);
    for (const t of alphas) expect(t.alpha, t.text).toBeGreaterThanOrEqual(t.struck ? 0.5 : 0.6);
    for (const el of root().querySelectorAll<HTMLElement>("*")) expect(el.style.backdropFilter).toBe("");
  });

  it("idle: the legacy surface is near opaque and unblurred", async () => {
    await renderPill("default");
    const capsule = await waitFor(() => {
      const el = root().querySelector<HTMLElement>("div[style*='transition']");
      expect(el).toBeTruthy();
      return el!;
    });
    expect(capsule.style.backdropFilter).toBe("");
    expect(capsule.style.background).toMatch(/0\.92\)/);
  });
});

describe("L3: pill controls are named for assistive tech", () => {
  it("the record dot is labelled, and says so for a command take", async () => {
    await renderPill("default");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    expect(await screen.findByRole("button", { name: "Stop recording" })).toBeTruthy();
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: true });
    expect(await screen.findByRole("button", { name: "Stop recording command" })).toBeTruthy();
  });

  it("the mic chip is a listbox trigger and the list is a listbox of options", async () => {
    await renderPill("default", { list_input_devices: DEVICES });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await screen.findByRole("button", { name: "Stop recording" });
    hoverPill();
    const chip = (await screen.findByText("MacBook")).closest("button")!;
    expect(chip.getAttribute("aria-haspopup")).toBe("listbox");
    expect(chip.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(chip);
    const list = await screen.findByRole("listbox", { name: "Microphone" });
    expect(chip.getAttribute("aria-expanded")).toBe("true");
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(2);
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["true", "false"]);
    expect(list.contains(options[0])).toBe(true);
  });

  it("the well's mic chip reports its expanded state", async () => {
    await renderPill("well", { list_input_devices: DEVICES });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    const chip = (await screen.findByText("MacBook")).closest("button")!;
    expect(chip.getAttribute("aria-haspopup")).toBe("listbox");
    expect(chip.getAttribute("aria-expanded")).toBe("false");
  });

  it("the polish card body is a button and the summary a separate status", async () => {
    await renderPill("classic", { open_main_window: undefined });
    await emitTauriEvent("polish-diff", { raw: "um hello there", polished: "Hello there.", ms: 900 });
    const status = await screen.findByRole("status");
    expect(status.textContent).toBe("Inserted. Polish made 1 change.");
    const body = screen.getByRole("button", { name: /click to open History/ });
    fireEvent.click(body);
    await waitFor(() => {
      expect(invokeCalls()).toContainEqual({ cmd: "open_main_window", args: { route: "home" } });
    });
  });
});

describe("L4: Classic hover neither shifts the bars nor darkens a patch", () => {
  it("keeps the clock slot and lips layout, and adds no second surface", async () => {
    await renderPill("classic");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: true });
    const clock = await screen.findByText(/^0:0\d$/);
    const lips = root().querySelector<HTMLElement>("[data-classic-lips]")!;
    const before = { flex: lips.style.flex, minWidth: lips.style.minWidth, nodes: lips.querySelectorAll("*").length };

    hoverPill();
    await screen.findByRole("button", { name: "Cancel" });
    // Same element, still laid out, only hidden: the lips cannot grow.
    expect(screen.getByText(/^0:0\d$/)).toBe(clock);
    expect(clock.style.visibility).toBe("hidden");
    expect(screen.getByTitle("Command take").parentElement!.style.visibility).toBe("hidden");
    expect({ flex: lips.style.flex, minWidth: lips.style.minWidth, nodes: lips.querySelectorAll("*").length }).toEqual(before);
    expect(lips.style.maskImage).toContain("linear-gradient");

    // Only the capsule itself carries the dark surface; the Cancel wrapper
    // has no backdrop or ring of its own.
    const surfaces = [...root().querySelectorAll<HTMLElement>("*")].filter((el) =>
      /rgba\(20,\s*20,\s*28/.test(el.style.background),
    );
    expect(surfaces).toHaveLength(1);
    const wrapper = screen.getByRole("button", { name: "Cancel" }).parentElement!;
    expect(wrapper.style.boxShadow).toBe("");
    expect(wrapper.style.background).toBe("");
  });
});

describe("L6: Level Rail bars fill the capsule when there is no text", () => {
  it("spreads 24 bars across the row without resizing the capsule", async () => {
    await renderPill("levelRail");
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await waitFor(() => expect(root().querySelector("[data-rail-meter]")).toBeTruthy());
    const meter = root().querySelector<HTMLElement>("[data-rail-meter]")!;
    expect(meter.children).toHaveLength(24);
    expect(meter.style.flex).toBe("1 1 auto");
    expect(meter.style.justifyContent).toBe("space-between");
    const bar = meter.children[0] as HTMLElement;
    expect(bar.style.flex).toBe("1 1 0px");
    expect(bar.style.maxWidth).toBe("4px");
    expect((meter.parentElement as HTMLElement).style.width).toBe("288px");

    await emitTauriEvent("interim-transcript", "hello there");
    await screen.findByTitle("hello there");
    expect((meter.parentElement as HTMLElement).style.width).toBe("288px");
  });
});

describe("review fixes: Cancel's disarm timer and the legacy busy cue", () => {
  it("a stale disarm timer from an earlier arm never disarms a later one", async () => {
    await renderPill("classic", { cancel_dictation: undefined });
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    await screen.findByText("0:00");
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    hoverPill();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Sure?" })).toBeTruthy();
    // Hover ends: the button unmounts, and an armed Cancel disarms with it.
    fireEvent.mouseLeave(root().firstElementChild!.firstElementChild!);
    hoverPill();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    // 2.5s after the first arm: its timer must be gone.
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(screen.getByRole("button", { name: "Sure?" })).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
  });

  it("a legacy busy pill keeps a looping cue while work is in progress", async () => {
    const breathing = () => root().querySelector<HTMLElement>("[style*='hairline-breathe']");
    await renderPill("default");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await screen.findByText("Transcribing…");
    expect(breathing()).not.toBeNull();
    setPhase({ phase: "processing", message: "Pasted" });
    await screen.findByText("Pasted");
    expect(breathing()).toBeNull();
  });

  it("the cue holds still under reduced motion", async () => {
    setReducedMotion(true);
    await renderPill("classic");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await screen.findByText("Transcribing…");
    expect(root().querySelector("[style*='hairline-breathe']")).toBeNull();
  });
});
