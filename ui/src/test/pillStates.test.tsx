import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import {
  emitTauriEvent,
  invokeCalls,
  listenHandlersFor,
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
} from "./tauriMock";

// Shared mock instances created with vi.hoisted so they survive
// vi.resetModules(): the pill re-imports @tauri-apps/api/* within each test,
// and a plain `vi.fn()` factory hands it a FRESH function per evaluation -
// invisible to the tauriMock capture helpers, which patch the instance
// captured at module load. Hoisted instances keep both views identical.
const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

// jsdom has no ResizeObserver; the pill's size reporter (pill.tsx ~1231)
// constructs one in a mount effect, and the sync throw unmounts the whole
// React root. A no-op class keeps the mount alive - these tests assert the
// pill's affordances, not its window sizing.
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
vi.stubGlobal("ResizeObserver", ResizeObserverStub);

// The pill entry point renders itself into #root on import (createRoot in
// pillEntry.tsx), so the test provides a #root container before importing the
// module. The module-level render is the component under test; we drive it
// through the `pill-state` backend event the app truly listens for.
// Phase payload shapes mirror PillPhase in pill.tsx:
//   { phase: "idle" } | { phase: "recording"; startedAtMs; command }
//   | { phase: "warming" } | { phase: "processing"; message }
type PillPhase =
  | { phase: "idle" }
  | { phase: "recording"; startedAtMs: number; command: boolean }
  | { phase: "warming" }
  | { phase: "processing"; message: string };

function setPhase(phase: PillPhase): void {
  void emitTauriEvent<PillPhase>("pill-state", phase);
}

describe("Pill state machine (driven via the pill-state backend event)", () => {
  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("renders idle bars on initial mount and subscribes to the backend events", async () => {
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

    // The real event names the pill listens for (pill.tsx useTauriEvent calls).
    // Mount effects flush after the import promise resolves: wait for the
    // subscription instead of asserting on the very next microtask.
    await waitFor(() => {
      expect(listenHandlersFor("pill-state")).toHaveLength(1);
    });
    expect(listenHandlersFor("settings-changed")).toHaveLength(1);
    // The idle affordance is a bar-only capsule: no visible text at all.
    expect(document.getElementById("root")!.textContent).toBe("");
    // It re-reads settings on mount.
    expect(invokeCalls().map((c) => c.cmd)).toContain("get_settings");
  });

  it("shows the recording affordances (red dot + clock) for a normal take", async () => {
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
      expect(listenHandlersFor("pill-state")).toHaveLength(1);
    });

    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });

    // RecordingDot carries the stop affordance; the clock renders 0:00.
    await waitFor(() => {
      expect(
        screen.getByTitle("Recording. Click or press your hotkey to stop.")
      ).toBeTruthy();
    });
    expect(document.getElementById("root")!.textContent).toContain("0:00");
  });
});

describe("Pill phase affordances (well style: LISTENING/COMMAND badges)", () => {
  // In pillStyle "well" the recording pill shows the LISTENING/COMMAND badge
  // and the "Listening…" / "Speak your command…" well text — the real
  // affordances for the listening and command-takeover phases.
  async function renderPill(pillStyle: string) {
    mockInvokeResponses({
      get_settings: {
        recordingMode: "hold",
        language: "en",
        inputDevice: "",
        pillStyle,
      },
      list_input_devices: [],
    });
    mockListenCapture();
    document.body.innerHTML = '<div id="root"></div>';
    vi.resetModules();
    await import("../pillEntry");
    // The pill mounts and registers its event listeners in a React effect,
    // which flushes after the import promise resolves: wait for the
    // subscription before emitting any phase.
    await waitFor(() => {
      expect(listenHandlersFor("pill-state")).toHaveLength(1);
    });
  }

  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("listening: LISTENING badge + 'Listening…' well text", async () => {
    await renderPill("well");

    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });

    await waitFor(() => {
      expect(screen.getByText("LISTENING")).toBeTruthy();
    });
    expect(screen.getByText("Listening…")).toBeTruthy();
  });

  it("command takeover: COMMAND badge + 'Speak your command…' well text", async () => {
    await renderPill("well");

    setPhase({ phase: "recording", startedAtMs: Date.now(), command: true });

    await waitFor(() => {
      expect(screen.getByText("COMMAND")).toBeTruthy();
    });
    expect(screen.getByText("Speak your command…")).toBeTruthy();
  });

  it("warming: 'Getting model ready…' status with the esc affordance", async () => {
    await renderPill("default");

    setPhase({ phase: "warming" });

    await waitFor(() => {
      expect(screen.getByText("Getting model ready…")).toBeTruthy();
    });
    expect(screen.getByText("esc")).toBeTruthy();
  });

  it("warming with model-load-progress: shows the download percentage", async () => {
    await renderPill("default");

    setPhase({ phase: "warming" });
    await waitFor(() => {
      expect(screen.getByText("Getting model ready…")).toBeTruthy();
    });

    await emitTauriEvent<number>("model-load-progress", 0.42);
    await waitFor(() => {
      expect(screen.getByText("Downloading model 42%")).toBeTruthy();
    });
    // The generic fallback is gone once a fraction is known.
    expect(screen.queryByText("Getting model ready…")).toBeNull();
  });

  it("warming: a late model-load-progress after the take ended does not linger", async () => {
    await renderPill("default");

    setPhase({ phase: "warming" });
    await waitFor(() => {
      expect(screen.getByText("Getting model ready…")).toBeTruthy();
    });
    await emitTauriEvent<number>("model-load-progress", 0.7);
    await waitFor(() => {
      expect(screen.getByText("Downloading model 70%")).toBeTruthy();
    });

    // Warm-up done: the fraction must not survive into the next phase.
    setPhase({ phase: "processing", message: "Transcribing…" });
    await waitFor(() => {
      expect(screen.getByText("Transcribing…")).toBeTruthy();
    });
    expect(screen.queryByText("Downloading model 70%")).toBeNull();
  });
});

describe("Pill done and error-path affordances", () => {
  async function renderPill(pillStyle: string) {
    mockInvokeResponses({
      get_settings: {
        recordingMode: "hold",
        language: "en",
        inputDevice: "",
        pillStyle,
      },
      list_input_devices: [],
    });
    mockListenCapture();
    document.body.innerHTML = '<div id="root"></div>';
    vi.resetModules();
    await import("../pillEntry");
    // The pill mounts and registers its event listeners in a React effect,
    // which flushes after the import promise resolves: wait for the
    // subscription before emitting any phase.
    await waitFor(() => {
      expect(listenHandlersFor("pill-state")).toHaveLength(1);
    });
  }

  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("processing (done path): renders the backend-provided status message", async () => {
    await renderPill("default");

    // The processing phase IS the done affordance: the backend's UiState
    // message is shown verbatim while the result is inserted.
    setPhase({ phase: "processing", message: "Transforming…" });

    await waitFor(() => {
      expect(screen.getByText("Transforming…")).toBeTruthy();
    });
    // And it returns to idle when the backend emits idle.
    setPhase({ phase: "idle" });
    await waitFor(() => {
      expect(
        document.getElementById("root")!.textContent
      ).not.toContain("Transforming…");
    });
  });

  it("pill-skip: the transform-skip message surfaces on the processing pill", async () => {
    await renderPill("default");

    setPhase({ phase: "processing", message: "Transforming…" });
    await emitTauriEvent<{ message: string }>("pill-skip", {
      message: "Kept your words as-is",
    });

    await waitFor(() => {
      expect(screen.getByText("Kept your words as-is")).toBeTruthy();
    });
  });

  it("mic failure: names it and opens microphone settings from the pill", async () => {
    await renderPill("default");

    setPhase({ phase: "processing", message: "No audio captured" });
    await emitTauriEvent<void>("mic-failure", undefined);

    const action = await screen.findByRole("button", { name: "Open Settings" });
    fireEvent.click(action);

    await waitFor(() => {
      expect(invokeCalls()).toContainEqual({
        cmd: "open_permission_settings",
        args: { kind: "microphone" },
      });
    });
  });

  it("mic failure: an ordinary status message never shows the shortcut", async () => {
    await renderPill("default");

    setPhase({ phase: "processing", message: "Transforming…" });

    await waitFor(() => {
      expect(screen.getByText("Transforming…")).toBeTruthy();
    });
    expect(screen.queryByRole("button", { name: "Open Settings" })).toBeNull();
  });

  it("T10a: a model-load failure renders the error variant with the settings action", async () => {
    await renderPill("default");

    setPhase({
      phase: "processing",
      message: "Couldn't load the speech model (missing file)",
    });

    await waitFor(() => {
      expect(
        screen.getByText("Couldn't load the speech model (missing file)"),
      ).toBeTruthy();
    });
    // The error body offers the recovery action (Open Settings).
    expect(
      await screen.findByRole("button", { name: "Open Settings" }),
    ).toBeTruthy();
  });

  it("T10a: a denied mic permission renders the error variant with the mic shortcut", async () => {
    await renderPill("default");

    setPhase({ phase: "processing", message: "Microphone access is off" });
    await emitTauriEvent<void>("mic-failure", undefined);

    await waitFor(() => {
      expect(screen.getByText("Microphone access is off")).toBeTruthy();
    });
    // The mic-failure event carries the Open Settings shortcut that opens
    // the OS microphone permission pane.
    const action = await screen.findByRole("button", { name: "Open Settings" });
    fireEvent.click(action);

    await waitFor(() => {
      expect(invokeCalls()).toContainEqual({
        cmd: "open_permission_settings",
        args: { kind: "microphone" },
      });
    });
  });
});

describe("Pill phase affordances (glow style)", () => {
  // jsdom has no matchMedia; the glow pill reads the OS motion, contrast and
  // transparency preferences through it.
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));

  async function renderPill() {
    mockInvokeResponses({
      get_settings: { recordingMode: "hold", language: "en", inputDevice: "", pillStyle: "glow" },
      list_input_devices: [],
    });
    mockListenCapture();
    document.body.innerHTML = '<div id="root"></div>';
    vi.resetModules();
    await import("../pillEntry");
    await waitFor(() => {
      expect(listenHandlersFor("pill-state")).toHaveLength(1);
      expect(invokeCalls().map((c) => c.cmd)).toContain("list_input_devices");
    });
  }

  function frame(): HTMLElement | null {
    return document.querySelector<HTMLElement>("[data-glow-frame]");
  }

  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("recording: the capsule with a labelled stop dot", async () => {
    await renderPill();
    setPhase({ phase: "recording", startedAtMs: Date.now(), command: false });
    expect(await screen.findByRole("button", { name: "Stop recording" })).toBeTruthy();
    expect(frame()!.querySelectorAll("[data-glow-bar]")).toHaveLength(11);
  });

  it("warming: the warming capsule with the status text", async () => {
    await renderPill();
    setPhase({ phase: "warming" });
    expect(await screen.findByText("Getting model ready…")).toBeTruthy();
    expect(frame()!.querySelector("[data-glow-warming]")).not.toBeNull();
  });

  it("transcribing: the working circle, named in the live region", async () => {
    await renderPill();
    setPhase({ phase: "processing", message: "Transcribing…" });
    await waitFor(() => expect(frame()?.querySelector("[data-glow-working]")).not.toBeNull());
    expect(frame()!.querySelector('[role="status"]')!.textContent).toBe("Transcribing");
  });

  it("done notes: Pasted, Too short and Cancelled render as notes", async () => {
    await renderPill();
    for (const [message, tone] of [["Pasted", "success"], ["Too short", "warn"], ["Cancelled", "neutral"]]) {
      setPhase({ phase: "processing", message });
      await waitFor(() => expect(frame()?.querySelector(`[data-glow-note="${tone}"]`)).not.toBeNull());
      // The note text (its title carries the full string) plus the same words
      // in the polite live region.
      expect(screen.getByTitle(message).textContent).toBe(message);
      expect(frame()!.querySelector('[role="status"]')!.textContent).toBe(message);
    }
  });

  it("skip: the note stacks the skip line under the status", async () => {
    await renderPill();
    setPhase({ phase: "processing", message: "Transforming…" });
    await emitTauriEvent<{ message: string }>("pill-skip", { message: "Kept your words as-is" });
    expect(await screen.findByText("Kept your words as-is")).toBeTruthy();
    expect(screen.getByText("Transforming…")).toBeTruthy();
  });

  it("error: a model-load failure offers Settings in an alert", async () => {
    await renderPill();
    setPhase({ phase: "processing", message: "Couldn't load the speech model (missing file)" });
    expect((await screen.findByRole("alert")).textContent).toBe("Couldn't load the speech model (missing file)");
    expect(screen.getByRole("button", { name: "Open Settings" })).toBeTruthy();
  });

  it("mic failure: the note opens microphone settings", async () => {
    await renderPill();
    setPhase({ phase: "processing", message: "No audio captured" });
    await emitTauriEvent<void>("mic-failure", undefined);
    fireEvent.click(await screen.findByRole("button", { name: "Open Settings" }));
    await waitFor(() => {
      expect(invokeCalls()).toContainEqual({ cmd: "open_permission_settings", args: { kind: "microphone" } });
    });
  });

  it("idle: glow leaves the idle bars to the shared chrome", async () => {
    await renderPill();
    setPhase({ phase: "idle" });
    await waitFor(() => expect(document.getElementById("root")!.textContent).toBe(""));
    expect(frame()).toBeNull();
  });
});
