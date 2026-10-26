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

  it("stays hidden when polish changed nothing", async () => {
    await mountPill();
    await emitTauriEvent("polish-diff", {
      raw: "same words",
      polished: "same words",
      ms: 200,
    });
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByText("Configure Polish")).toBeNull();
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
