import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { RainbowLips } from "../pill";
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

async function renderRecordingPill(pillStyle: string): Promise<void> {
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
  });
  emitTauriEvent("pill-state", { phase: "recording", startedAtMs: Date.now() });
  // Recording entry resets the level history, so feed full-scale levels
  // AFTER the phase flip and enough of them to fill the whole window
  // (the display curve maps 1 -> 1, so bars reach their geometry maximum).
  await waitFor(() => expect(document.body.textContent).toContain("0:0"));
  for (let i = 0; i < 80; i++) emitTauriEvent("pill-level", 1);
  // Flush the batched state updates so the measured DOM reflects the levels.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });
}

function barHeights(): number[] {
  return [...document.querySelectorAll("span")]
    .map((s) => parseFloat((s as HTMLElement).style.height) || 0)
    .filter((h) => h > 0);
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

describe("classic lips waveform", () => {
  it("varies bar heights with the level history instead of drawing a flat comb", () => {
    const wave = [0.1, 0.4, 0.9, 0.5, 0.2, 0.8, 1, 0.6, 0.3, 0.7, 0.9, 0.4, 0.1, 0.5, 0.8, 0.6, 0.2, 0.7];
    const { container } = render(<RainbowLips levels={wave} />);
    const heights = [...container.querySelectorAll("span")]
      .map((s) => parseFloat((s as HTMLElement).style.height))
      .filter((h) => h > 0);

    expect(heights).toHaveLength(18);
    expect(new Set(heights).size).toBeGreaterThan(5);
    expect(Math.max(...heights) - Math.min(...heights)).toBeGreaterThan(10);
  });

  it("renders the denser 26-bar capsule row", () => {
    const wave = new Array(26).fill(0.6);
    const { container } = render(<RainbowLips levels={wave} bars={26} />);
    expect(container.querySelectorAll("span")).toHaveLength(26);
  });
});

describe("recording waveform geometry (height-wise)", () => {
  it("classic capsule lips reach at least 23px at full level", async () => {
    await renderRecordingPill("classic");
    await waitFor(() => expect(barHeights().length).toBeGreaterThanOrEqual(24));
    expect(Math.max(...barHeights())).toBeGreaterThanOrEqual(23);
  });

  it("level rail bars reach at least 26px at full level", async () => {
    await renderRecordingPill("levelRail");
    await waitFor(() => expect(barHeights().length).toBeGreaterThan(10));
    expect(Math.max(...barHeights())).toBeGreaterThanOrEqual(26);
  });

  it("reading well meter reaches at least 24px at full level", async () => {
    await renderRecordingPill("well");
    await waitFor(() => expect(barHeights().length).toBeGreaterThan(10));
    expect(Math.max(...barHeights())).toBeGreaterThanOrEqual(24);
  });
});
