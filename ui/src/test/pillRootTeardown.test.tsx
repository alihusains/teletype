// claim: ui-behavior
// Regression suite for the pill teardown contract (UI-FLAKE-3).
//
// The hosted CI runner fails the UI job with an unhandled
// "ReferenceError: window is not defined" while all 530 tests pass. The error
// comes from the recording-phase timer in useNow.ts firing into a React state
// update after jsdom has been torn down. That timer belongs to a root mounted by
// ui/pillEntry.tsx, which testing-library's auto-cleanup cannot reach because it
// only unmounts roots it created itself. Locally the suite is green either way:
// the interval only lands after teardown when the runner is slow enough, so
// these tests pin the contract structurally instead of racing it.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "@testing-library/react";
import { resetTauriMocks } from "./tauriMock";
import { renderPill, resetPillHarness, setPhase } from "./pillHarness";
import { unmountAllPillRoots } from "../dev/pillRootRegistry";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

function trackedRoots(): (() => void)[] {
  return globalThis.__teletypePillUnmounts ?? [];
}

afterEach(() => {
  resetTauriMocks();
  resetPillHarness();
});

describe("pill root teardown", () => {
  it("tracks the root the pill entry point mounts", async () => {
    await renderPill("default");
    // This is the leak: an entry-mounted root is invisible to every other
    // cleanup, so if tracking ever stops happening the CI error comes back.
    expect(trackedRoots()).toHaveLength(1);
    // Deliberately no unmount here. The next test has to start from the state
    // this one leaves behind, otherwise an in-test unmount would satisfy it and
    // the teardown contract would go unchecked.
  });

  it("starts with an empty registry because the suite teardown drains it", async () => {
    // The test above leaves its root tracked. If src/test/setup.ts did not
    // unmount in an afterEach, that root would still be alive here with its
    // recording timer pending, which is the state CI tears down mid-fire.
    expect(trackedRoots()).toHaveLength(0);
    await renderPill("glow");
    expect(trackedRoots()).toHaveLength(1);
  });

  it("clears the recording-phase timer when the tracked root unmounts", async () => {
    await renderPill("default");
    // useNow arms its interval with the phase as the delay (pill.tsx): 500ms in
    // the recording phase, which is the one that survives teardown in CI.
    const RECORDING_INTERVAL_MS = 500;
    const armed: number[] = [];
    const cleared: number[] = [];
    const realSet = globalThis.setInterval;
    const realClear = globalThis.clearInterval;
    vi.stubGlobal("setInterval", (fn: () => void, ms?: number, ...rest: unknown[]) => {
      const id = realSet(fn, ms, ...rest) as unknown as number;
      if (ms === RECORDING_INTERVAL_MS) armed.push(id);
      return id;
    });
    vi.stubGlobal("clearInterval", (id?: number) => {
      if (id !== undefined) cleared.push(id);
      return realClear(id);
    });

    await act(async () => {
      setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    });
    expect(armed).not.toHaveLength(0);

    unmountAllPillRoots();
    expect(cleared).toEqual(expect.arrayContaining(armed));
    expect(trackedRoots()).toHaveLength(0);

    vi.stubGlobal("setInterval", realSet);
    vi.stubGlobal("clearInterval", realClear);
  });
});
