// claim: ui-behavior
/**
 * Every screen stays mounted (App.tsx toggles `display`), so a hidden screen
 * must not keep doing background work. The Developer screen is the worst case:
 * it polls `get_logs` and `get_last_dictation_timings` every 500 ms. With
 * `active={false}` the initial tick and the interval must both be skipped;
 * flipping back to `active={true}` must restart the poll.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import DeveloperScreen from "../screens/DeveloperScreen";
import { ReadinessPanel } from "../lib/runtimeStatus";
import { invokeCalls, mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

function getLogsCallCount(): number {
  return invokeCalls().filter((c) => c.cmd === "get_logs").length;
}

function runtimeStatusCallCount(): number {
  return invokeCalls().filter((c) => c.cmd === "get_runtime_status").length;
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
  vi.useRealTimers();
});

describe("active-screen gate (ROADMAP §16 / L063)", () => {
  it("does not poll logs or timings while inactive, and restarts when active", () => {
    vi.useFakeTimers();
    mockListenCapture();
    mockInvokeResponses({
      get_profile: { s1Control: { styling: "casual", structure: "prose", context: "general" } },
      get_settings: { selectedLlmModel: "s1-mini" },
      get_logs: [],
      get_last_dictation_timings: null,
    });

    const { rerender } = render(<DeveloperScreen active={false} />);

    // The screen is hidden: no initial tick, no interval.
    vi.advanceTimersByTime(5000);
    expect(getLogsCallCount()).toBe(0);
    expect(invokeCalls().some((c) => c.cmd === "get_last_dictation_timings")).toBe(false);

    // Coming back to the tab restarts the poll: the initial tick fires at once
    // and the interval keeps firing.
    rerender(<DeveloperScreen active={true} />);
    expect(getLogsCallCount()).toBe(1);
    vi.advanceTimersByTime(1500);
    expect(getLogsCallCount()).toBe(4);
  });

  it("ReadinessPanel stops its 5 s poll while inactive and resumes when active", async () => {
    vi.useFakeTimers();
    mockListenCapture();
    mockInvokeResponses({
      get_runtime_status: {
        speech: { state: "ready", label: "Speech", modelId: "parakeet", detail: "Ready", blocking: false },
        llm: { state: "loaded", label: "LLM", modelId: "s1-mini", detail: "Loaded", blocking: false },
      },
    });

    const { rerender } = render(<ReadinessPanel status={null} active={false} />);
    await act(async () => {
      await Promise.resolve();
    });
    vi.advanceTimersByTime(5000);
    expect(runtimeStatusCallCount()).toBe(0);

    // Activating the owning screen reads at once, then resumes the cadence.
    // The poll timer is armed after the initial read resolves, so flush the
    // microtask before advancing fake time.
    await act(async () => {
      rerender(<ReadinessPanel status={null} active={true} />);
      await Promise.resolve();
    });
    expect(runtimeStatusCallCount()).toBe(1);
    await act(async () => {
      vi.advanceTimersByTime(5000);
      await Promise.resolve();
    });
    expect(runtimeStatusCallCount()).toBe(2);
  });

  it("polls normally when active from the start", () => {
    vi.useFakeTimers();
    mockListenCapture();
    mockInvokeResponses({
      get_profile: { s1Control: { styling: "casual", structure: "prose", context: "general" } },
      get_settings: { selectedLlmModel: "s1-mini" },
      get_logs: [],
      get_last_dictation_timings: null,
    });

    render(<DeveloperScreen />);
    expect(getLogsCallCount()).toBe(1);
    vi.advanceTimersByTime(500);
    expect(getLogsCallCount()).toBe(2);
  });
});
