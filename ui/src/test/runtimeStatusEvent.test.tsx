// claim: ui-behavior
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { useRuntimeStatus, checkedAgoLabel, type RuntimeStatus } from "../lib/runtimeStatus";
import {
  emitTauriEvent,
  invokeCalls,
  mockListenCapture,
  resetTauriMocks,
  type InvokeCall,
} from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";

const invokeFn = vi.mocked(invoke);

/** A component that only renders the hook's output. */
function Probe() {
  const { status, lastChecked, stale } = useRuntimeStatus();
  return (
    <div>
      <span data-testid="speech-state">{status?.speech.state ?? "null"}</span>
      <span data-testid="llm-state">{status?.llm.state ?? "null"}</span>
      <span data-testid="stale">{String(stale)}</span>
      <span data-testid="last-checked">{String(lastChecked)}</span>
    </div>
  );
}

const LOADING_PAYLOAD: RuntimeStatus = {
  speech: { state: "loading", label: "Speech", modelId: "parakeet", detail: "Loading model", blocking: true },
  llm: { state: "none", label: "LLM", modelId: "s1-mini", detail: "Not loaded", blocking: false },
};

const LOADED_PAYLOAD: RuntimeStatus = {
  speech: { state: "ready", label: "Speech", modelId: "parakeet", detail: "Ready", blocking: false },
  llm: { state: "loaded", label: "LLM", modelId: "s1-mini", detail: "Loaded", blocking: false },
};

function pollCalls(): InvokeCall[] {
  return invokeCalls().filter((c) => c.cmd === "get_runtime_status");
}

/** Flush all pending microtasks so the initial invoke settles. */
async function flush() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe("useRuntimeStatus event path", () => {
  beforeEach(() => {
    resetTauriMocks();
    mockListenCapture();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    resetTauriMocks();
  });

  it("applies the runtime-status event payload immediately, before the next poll", async () => {
    invokeFn.mockResolvedValue(LOADING_PAYLOAD);
    vi.useFakeTimers();

    render(<Probe />);
    await flush();

    // Initial poll resolved with loading.
    expect(screen.getByTestId("speech-state").textContent).toBe("loading");

    // Deliver the event.
    await act(async () => {
      await emitTauriEvent("runtime-status", LOADED_PAYLOAD);
    });

    // The event payload is visible immediately.
    expect(screen.getByTestId("speech-state").textContent).toBe("ready");
    expect(screen.getByTestId("llm-state").textContent).toBe("loaded");

    // Only one poll call so far (the initial read); the event did not trigger a new invoke.
    expect(pollCalls()).toHaveLength(1);
  });

  it("restarts the poll timer on event: no duplicate read just under POLL_MS, then a read just past", async () => {
    vi.useFakeTimers();
    invokeFn.mockResolvedValue(LOADING_PAYLOAD);

    render(<Probe />);
    await flush();

    // Initial read done.
    expect(pollCalls()).toHaveLength(1);

    // Deliver the event: this resets the poll timer.
    await act(async () => {
      await emitTauriEvent("runtime-status", LOADED_PAYLOAD);
    });

    // Advance just under 5 s: no second poll.
    await act(async () => {
      vi.advanceTimersByTime(4999);
    });
    expect(pollCalls()).toHaveLength(1);

    // Cross the 5 s boundary: the safety-net poll fires.
    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });
    expect(pollCalls()).toHaveLength(2);
  });

  it("keeps the last value and sets stale when a poll rejects", async () => {
    vi.useFakeTimers();
    invokeFn.mockResolvedValue(LOADED_PAYLOAD);

    render(<Probe />);
    await flush();

    // Good value is rendered.
    expect(screen.getByTestId("speech-state").textContent).toBe("ready");
    expect(screen.getByTestId("llm-state").textContent).toBe("loaded");
    expect(screen.getByTestId("stale").textContent).toBe("false");

    // Next poll rejects.
    invokeFn.mockRejectedValue(new Error("backend down"));

    // Advance past POLL_MS to trigger the next poll.
    await act(async () => {
      vi.advanceTimersByTime(5000);
      await Promise.resolve();
    });

    // Stale flag is true.
    expect(screen.getByTestId("stale").textContent).toBe("true");

    // The last good value is still rendered (not null, not cleared).
    expect(screen.getByTestId("speech-state").textContent).toBe("ready");
    expect(screen.getByTestId("llm-state").textContent).toBe("loaded");
  });

  it("sets stale when the initial poll rejects", async () => {
    vi.useFakeTimers();
    invokeFn.mockRejectedValue(new Error("no backend"));

    render(<Probe />);
    await flush();

    expect(screen.getByTestId("stale").textContent).toBe("true");
    expect(screen.getByTestId("speech-state").textContent).toBe("null");
  });
});

describe("checkedAgoLabel", () => {
  it("returns 'Not checked yet' for null", () => {
    expect(checkedAgoLabel(null, 100000)).toBe("Not checked yet");
  });

  it("returns 'Checked 3s ago' for a timestamp 3000 ms old", () => {
    const now = 100000;
    const lastChecked = now - 3000;
    expect(checkedAgoLabel(lastChecked, now)).toBe("Checked 3s ago");
  });

  it("clamps to 'Checked 1s ago' for a sub-second interval", () => {
    const now = 100000;
    const lastChecked = now - 100;
    expect(checkedAgoLabel(lastChecked, now)).toBe("Checked 1s ago");
  });
});
