// claim: ui-behavior
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
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

async function mountPill(extraResponses: Record<string, unknown> = {}): Promise<void> {
  mockInvokeResponses({
    get_settings: {
      recordingMode: "hold",
      language: "en",
      inputDevice: "",
      pillStyle: "default",
    },
    list_input_devices: [],
    record_polish_feedback: null,
    rewrite_with_transform: { text: "A stronger rewrite.", skipMessage: null },
    ...extraResponses,
  });
  mockListenCapture();
  document.body.innerHTML = '<div id="root"></div>';
  vi.resetModules();
  await import("../pillEntry");
  await waitFor(() => {
    expect(listenHandlersFor("polish-diff")).toHaveLength(1);
  });
}

describe("post-insertion polish card", () => {
  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("renders the diff after a take lands, then dismisses", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "one three",
      ms: 840,
    });

    await waitFor(() => expect(screen.getByText("1 change")).toBeTruthy());
    expect(screen.getByText(/0\.8s/)).toBeTruthy();
    expect(screen.getByText("Configure Polish")).toBeTruthy();
    expect(screen.getByText("Inserted · click to open History")).toBeTruthy();

    fireEvent.click(screen.getByLabelText("Dismiss"));
    await waitFor(() => expect(screen.queryByText("1 change")).toBeNull());
  });

  it("shows 0 changes when polish changed nothing", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "same words",
      polished: "same words",
      ms: 200,
    });
    // The card always shows after a take; with no changes it reads "0
    // changes" so the user can see the take landed.
    await waitFor(() => expect(screen.getByText("Configure Polish")).toBeTruthy());
    expect(screen.getAllByText("0 changes").length).toBeGreaterThan(0);
  });

  it("hands a card it will not render back to Rust", async () => {
    // Rust already made the hidden pill clickable and is waiting for this
    // card's size report; a dropped card must be dismissed by id. An empty
    // polished payload is the only case the reducer still drops.
    await mountPill();
    await emitTauriEvent("polish-diff", { raw: "same words", polished: "", ms: 200, card: 7 });
    await waitFor(() => {
      const call = mocks.invoke.mock.calls.find(([cmd]) => cmd === "dismiss_pill_result");
      expect(call?.[1]).toEqual({ card: 7 });
    });
    expect(screen.queryByText("Configure Polish")).toBeNull();
  });

  it("tags the card's size report with its id, so only that report shows the window", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      () => ({ x: 0, y: 0, top: 0, left: 0, right: 460, bottom: 140, width: 460, height: 140, toJSON: () => ({}) }) as DOMRect,
    );
    await mountPill();
    await emitTauriEvent("polish-diff", { raw: "one two", polished: "one three", ms: 300, card: 4 });
    await waitFor(() => expect(screen.getByText("1 change")).toBeTruthy());
    const reports = mocks.invoke.mock.calls.filter(([cmd]) => cmd === "set_pill_size").map(([, args]) => args);
    expect(reports).toContainEqual({ width: 460, height: 140, card: 4 });
    // Every non-card report leaves the id out.
    for (const args of reports) {
      if (args && typeof args === "object" && "card" in args) expect(args).toEqual({ width: 460, height: 140, card: 4 });
    }
    vi.restoreAllMocks();
  });

  it("opens the Polish settings from its link", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", { raw: "a b", polished: "a c", ms: 300 });
    await waitFor(() => expect(screen.getByText("Configure Polish")).toBeTruthy());
    fireEvent.click(screen.getByText("Configure Polish"));
    await waitFor(() => {
      const call = mocks.invoke.mock.calls.find(([cmd]) => cmd === "open_main_window");
      expect(call?.[1]).toEqual({ route: "transforms" });
    });
  });

  it("shows recovery and feedback actions without covering the diff", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", { raw: "one two", polished: "one three", ms: 300, card: 12 });
    await waitFor(() => expect(screen.getByText("1 change")).toBeTruthy());
    for (const label of ["Copy what I said", "Thumbs up", "Thumbs down", "Why"]) {
      expect(screen.getByRole("button", { name: label })).toBeTruthy();
    }
  });

  it("copies the raw take and says so honestly", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    await mountPill();
    await emitTauriEvent("polish-diff", { raw: "one two", polished: "one three", ms: 300, card: 12 });
    await waitFor(() => expect(screen.getByText("1 change")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Copy what I said" }));
    await waitFor(() =>
      expect(screen.getByText("Copied what you said — paste to restore")).toBeTruthy(),
    );
    expect(writeText).toHaveBeenCalledWith("one two");
  });

  it("records a thumb with the card id", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", { raw: "one two", polished: "one three", ms: 300, card: 12 });
    await waitFor(() => expect(screen.getByText("1 change")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Thumbs up" }));
    await waitFor(() => {
      const call = mocks.invoke.mock.calls.find(([cmd]) => cmd === "record_polish_feedback");
      expect(call?.[1]).toMatchObject({ feedback: { takeId: 12, kind: "thumb_up" } });
    });
    await waitFor(() => expect(screen.getByText("Saved locally")).toBeTruthy());
  });

  it("shows the real reason and profile in the Why dialog, never synthesized ones", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "one three",
      ms: 300,
      card: 12,
      profile: "structured",
      reason: "cleanup-only model; advisory note",
    });
    await waitFor(() => expect(screen.getByText("1 change")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Why" }));
    const dialog = screen.getByRole("dialog", { name: "Why this changed" });
    // Every value comes from the event; nothing is invented for display.
    expect(within(dialog).getByText(/Model: unknown · Profile: structured/)).toBeTruthy();
    expect(
      within(dialog).getByText(/Gate \/ fallback reason: cleanup-only model; advisory note/),
    ).toBeTruthy();
    expect(within(dialog).queryByText(/Profile: unknown/)).toBeNull();
    expect(within(dialog).queryByText(/none reported/)).toBeNull();
    expect(within(dialog).queryByText(/unavailable on this card/)).toBeNull();
  });

  it("shows honest fallbacks when the payload omits profile and reason", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", { raw: "one two", polished: "one three", ms: 300 });
    await waitFor(() => expect(screen.getByText("1 change")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Why" }));
    const dialog = screen.getByRole("dialog", { name: "Why this changed" });
    expect(within(dialog).getByText(/Profile: unknown/)).toBeTruthy();
    expect(within(dialog).getByText(/Gate \/ fallback reason: none reported/)).toBeTruthy();
  });

  it("attributes thumb and never-change feedback with the real profile", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "one three",
      ms: 300,
      card: 12,
      profile: "structured",
    });
    await waitFor(() => expect(screen.getByText("1 change")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Thumbs up" }));
    await waitFor(() => {
      const call = mocks.invoke.mock.calls.find(([cmd]) => cmd === "record_polish_feedback");
      expect(call?.[1]).toMatchObject({
        feedback: { takeId: 12, kind: "thumb_up", profile: "structured" },
      });
    });

    // Selecting a change reveals "Never change this"; that feedback carries
    // the same profile so a protected term is credited to the right contract.
    fireEvent.click(screen.getByRole("button", { name: "Inspect change: three" }));
    fireEvent.click(screen.getByRole("button", { name: "Never change this" }));
    await waitFor(() => {
      const calls = mocks.invoke.mock.calls
        .filter(([cmd]) => cmd === "record_polish_feedback")
        .map(([, args]) => args as { feedback: { kind: string; profile?: string } });
      expect(calls.some((a) => a.feedback.kind === "never_change" && a.feedback.profile === "structured")).toBe(true);
    });
  });

  it("labels a clean no-op polish run honestly", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "same words",
      polished: "same words",
      ms: 200,
      ran: true,
    });
    await waitFor(() =>
      expect(screen.getByText("Polish ran · 0 changes · already clean")).toBeTruthy(),
    );
  });

  it("labels a skipped polish run with its reason", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "one two",
      ms: 0,
      ran: false,
      reason: "gate: already clean",
    });
    await waitFor(() =>
      expect(screen.getByText("Polish skipped · gate: already clean")).toBeTruthy(),
    );
  });

  it("falls back to an honest no-reason label when a skipped run omits it", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "one two",
      ms: 0,
      ran: false,
    });
    await waitFor(() =>
      expect(screen.getByText("Polish skipped · no reason reported")).toBeTruthy(),
    );
  });

  it("shows the original after filler removal behind a keyboard-accessible toggle", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "hello there",
      polished: "Hello there.",
      original: "um, hello there",
      ms: 400,
      ran: true,
    });
    await waitFor(() => expect(screen.getByText("Configure Polish")).toBeTruthy());
    const toggle = screen.getByRole("button", { name: "View original" });
    expect(toggle.tagName).toBe("BUTTON");
    expect(screen.queryByText("Original after filler removal")).toBeNull();

    fireEvent.click(toggle);
    expect(screen.getByText("Original after filler removal")).toBeTruthy();
    expect(screen.getByText("um, hello there")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Hide original" }));
    expect(screen.queryByText("Original after filler removal")).toBeNull();
  });

  it("labels the fallback as raw transcript when original is missing", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", { raw: "raw take", polished: "Raw take.", ms: 300 });
    await waitFor(() => expect(screen.getByText("Configure Polish")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "View original" }));
    const label = screen.getByText("Raw transcript");
    expect(label.parentElement?.textContent).toContain("raw take");
  });

  it("shows the real model in the Why dialog when the event carries one", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "one three",
      ms: 300,
      ran: true,
      model: "llama3.2:3b-polish",
      profile: "structured",
    });
    await waitFor(() => expect(screen.getByText("Configure Polish")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Why" }));
    const dialog = screen.getByRole("dialog", { name: "Why this changed" });
    expect(within(dialog).getByText(/Model: llama3\.2:3b-polish · Profile: structured/)).toBeTruthy();
    expect(within(dialog).queryByText(/Model: unknown/)).toBeNull();
  });

  it("rewrites the polished text with the Transform command", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "One, two.",
      ms: 300,
      ran: true,
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Rewrite stronger" })).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Rewrite stronger" }));

    await waitFor(() => {
      const call = mocks.invoke.mock.calls.find(([cmd]) => cmd === "rewrite_with_transform");
      expect(call?.[1]).toEqual({ text: "One, two." });
    });
    await waitFor(() => expect(screen.getByText("A stronger rewrite.")).toBeTruthy());
  });

  it("shows the rewrite's skip message verbatim and copies the rewrite", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    await mountPill({
      rewrite_with_transform: {
        text: "Rewritten stronger.",
        skipMessage: "model unavailable; showing draft",
      },
    });
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "one three",
      ms: 300,
      ran: true,
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Rewrite stronger" })).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Rewrite stronger" }));

    await waitFor(() => expect(screen.getByText("Rewritten stronger.")).toBeTruthy());
    expect(screen.getByText("model unavailable; showing draft")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Copy rewrite" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Copied" })).toBeTruthy(),
    );
    expect(writeText).toHaveBeenCalledWith("Rewritten stronger.");
  });

  it("reports an honest failure when the rewrite command throws", async () => {
    await mountPill({
      rewrite_with_transform: () => {
        throw new Error("backend down");
      },
    });
    await emitTauriEvent("polish-diff", {
      raw: "one two",
      polished: "one three",
      ms: 300,
      ran: true,
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Rewrite stronger" })).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Rewrite stronger" }));
    await waitFor(() => expect(screen.getByText("Rewrite failed")).toBeTruthy());
    expect(screen.queryByText("A stronger rewrite.")).toBeNull();
  });
});
