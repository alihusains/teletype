// claim: ui-behavior
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
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

async function mountPill(): Promise<void> {
  mockInvokeResponses({
    get_settings: {
      recordingMode: "hold",
      language: "en",
      inputDevice: "",
      pillStyle: "default",
    },
    list_input_devices: [],
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
});
