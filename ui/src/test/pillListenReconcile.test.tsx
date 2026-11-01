// claim: ui-behavior
//
// PILL-LISTEN-1b: the pill subscribes to backend events through
// useTauriEvent, whose subscription happens inside useEffect via the async
// listen() promise. A backend event emitted before that promise resolves is
// dropped by the subscriber, because no handler is registered yet. The pill
// machine seeds from initialPillMachineState, so a take that starts while
// the `pill-state` listener is still pending leaves the pill showing a
// stale phase until the next event arrives.
//
// The fix reconciles against the backend's current phase, which the existing
// `get_dictation_state` command returns (wire shape `UiState`,
// crates/teletype-core/src/state.rs: tag "phase", rename_all = "camelCase").
// The mock below returns only shapes that enum can produce, so the test
// cannot pass on a field the backend does not send.
//
// The `Message { text }` payload carries dictated text, which this test must
// never print or log; the stray-phase test below uses a fixed placeholder
// and asserts only that the machine did not move.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { usePillMachine } from "../pillMachine";
import { invokeCalls, mockInvokeResponses, resetTauriMocks } from "./tauriMock";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

/** The phase the backend is actually in when the take starts. The wire
 * shape is exactly what `get_dictation_state` returns: `UiState` with
 * internal tagging, `{"phase":"transcribing"}`. */
const BACKEND_STATE = { phase: "transcribing" };

/** Renders the machine's phase, so the test asserts on the state the
 * machine actually holds. */
function Probe() {
  const { machine } = usePillMachine();
  const pill = machine.pill as { phase: string };
  return <div data-testid="pill-phase">{pill.phase}</div>;
}

/** Gate every subscription behind one promise so the test controls the
 * exact moment the listeners register. */
function gateListen() {
  let openGate: () => void = () => {};
  const gate = new Promise<void>((r) => {
    openGate = r;
  });
  const registered: Array<{ event: string; deliver: (payload: unknown) => void }> = [];
  mocks.listen.mockImplementation(
    (event: string, handler: (e: { event: string; id: number; payload: unknown }) => void) => {
      return gate.then(() => {
        registered.push({
          event,
          deliver: (payload) => handler({ event: event as string, id: 0, payload }),
        });
        return () => {};
      });
    },
  );
  return { openGate, gate, registered };
}

describe("pill machine: listener-pending event loss (PILL-LISTEN-1b)", () => {
  beforeEach(() => {
    resetTauriMocks();
  });

  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("reaches the backend phase after the pill-state listener resolves", async () => {
    mockInvokeResponses({
      // Wire-accurate settings read: the Rust Settings struct has no phase
      // field, so the mock must not invent one.
      get_settings: {
        recordingMode: "hold",
        language: "en",
        inputDevice: "",
        pillStyle: "default",
      },
      list_input_devices: [],
      // The backend is mid-take when the pill mounts.
      get_dictation_state: BACKEND_STATE,
    });

    const { openGate, gate, registered } = gateListen();

    render(<Probe />);

    // The mount effects have run: the machine asked for every subscription,
    // but none is registered yet because the gate is still closed.
    await waitFor(() => {
      expect(mocks.listen.mock.calls.some(([e]) => e === "pill-state")).toBe(true);
    });
    expect(registered).toHaveLength(0);

    // The backend emits the current phase while the listener is pending.
    // No handler is registered yet, so this delivery reaches nobody: the
    // event is dropped, exactly like the production race.
    await act(async () => {
      for (const r of [...registered]) {
        if (r.event === "pill-state") r.deliver(BACKEND_STATE);
      }
    });

    // Let the subscriptions register. No further delivery: the backend
    // emitted its phase once, while the listener was pending, and it does
    // not re-emit state it has not changed. The only delivery the
    // subscriber ever gets is the one that was dropped above.
    await act(async () => {
      openGate();
      await gate;
    });

    // Expected behaviour: once the listener is live, the machine is in the
    // phase the backend is actually in. The reconcile asks the backend
    // through get_dictation_state and applies the answer through the
    // dictation-state reducer path.
    await waitFor(() => {
      expect(screen.getByTestId("pill-phase").textContent).toBe("transcribing");
    });

    // The reconcile must have asked the backend for the current phase.
    expect(invokeCalls().map((c) => c.cmd)).toContain("get_dictation_state");
  });

  it("a stray phase key inside a settings read does not move the machine", async () => {
    // The rejected PILL-LISTEN-1 approach read machine.settings.phase. The
    // Rust Settings struct has no phase field, so a settings payload with a
    // phase key is not something the backend can send. This test pins the
    // guard: even if such a payload arrived, the machine must stay at its
    // seed phase. The mock's get_dictation_state returns the idle shape, so
    // the reconcile itself also does nothing.
    mockInvokeResponses({
      get_settings: {
        recordingMode: "hold",
        language: "en",
        inputDevice: "",
        pillStyle: "default",
        // Stray key: not part of the Rust Settings wire shape.
        phase: "transcribing",
      } as unknown as Record<string, unknown>,
      list_input_devices: [],
      get_dictation_state: { phase: "idle" },
    });

    mockListenCaptureImmediate();
    render(<Probe />);

    await waitFor(() => {
      expect(mocks.listen.mock.calls.some(([e]) => e === "pill-state")).toBe(true);
    });
    // Flush the settings-loaded dispatch and the reconcile.
    await new Promise((r) => setTimeout(r, 0));

    // The machine stays at the seed phase: the stray settings key is not a
    // wire field, and the reconcile read the real backend phase (idle).
    expect(screen.getByTestId("pill-phase").textContent).toBe("idle");
  });
});

// The stray-phase test does not need the gate machinery: register
// subscriptions immediately (no pending window in that scenario).
function mockListenCaptureImmediate() {
  mocks.listen.mockImplementation(
    (_event: string, _handler: (e: { event: string; id: number; payload: unknown }) => void) => {
      return Promise.resolve(() => {});
    },
  );
}
