// claim: ui-behavior
/**
 * The Dictation screen's failure states (audit docs/app-states-audit.md):
 *
 * A-3  a rejected `list_dictation_history` renders the LoadFailed row, never
 *      the "No dictations yet" empty state; a successful empty read does.
 * A-1  a rejected delete keeps the row and renders a per-row write error;
 *      the delete is gated behind confirmDialog.
 * A-10 a rejected clipboard write does NOT flip the "Copied" affordance.
 * A-4  three files dropped at once queue three jobs and the busy label names
 *      the position ("2 of 3"), not just the last file.
 * A-11 the Choose-file button and the drag handler share one busy gate.
 */
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import DictationScreen from "../screens/DictationScreen";
import { mockInvokeResponses, invokeCalls, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
  message: vi.fn(),
}));

// The drag-drop and focus refresh subscribe through the webview window.
// onDragDropEvent is a spy so tests can replay drop payloads; the handler
// must be registered before the first replay (the component subscribes in a
// mount effect, so wait for the list of drops to settle).
const webviewMock = vi.hoisted(() => ({
  onDragDropEvent: vi.fn((handler: (e: { payload: { type: string; paths: string[] } }) => void) => {
    webviewMock.dropHandler = handler;
    return Promise.resolve(() => {});
  }),
  onFocusChanged: vi.fn(() => Promise.resolve(() => {})),
  dropHandler: undefined as
    | ((e: { payload: { type: string; paths: string[] } }) => void)
    | undefined,
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onDragDropEvent: webviewMock.onDragDropEvent,
    onFocusChanged: webviewMock.onFocusChanged,
  }),
}));

/** Replay a native drop through the handler the component registered. */
function dropPaths(paths: string[]): void {
  webviewMock.dropHandler?.({ payload: { type: "drop", paths } });
}

function entry(over: Partial<{ id: string; text: string; appName: string; appType: string }> = {}) {
  return {
    id: "e1",
    createdAt: Date.now(),
    text: "Hello from a dictation",
    appName: "Mail",
    appType: "Email",
    ...over,
  };
}

/** Swap navigator.clipboard.writeText for a resolving or rejecting stub. */
function patchClipboard(allow: boolean) {
  const writeText = vi.fn(allow ? () => Promise.resolve() : () => Promise.reject(new Error("denied")));
  vi.stubGlobal("navigator", { clipboard: { writeText } } as never);
  return writeText;
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
  webviewMock.dropHandler = undefined;
  vi.mocked(webviewMock.onDragDropEvent).mockClear();
  vi.mocked(webviewMock.onFocusChanged).mockClear();
  vi.mocked(confirm).mockReset();
  vi.mocked(open).mockReset();
  vi.unstubAllGlobals();
});

// confirmDialog (ui/src/lib/dialogs.ts) only routes to the Tauri dialog
// plugin when it detects the Tauri runtime via __TAURI_INTERNALS__. jsdom
// doesn't set that, so without this the confirm gate falls through to the
// native window.confirm (which jsdom stubs to false) and the mocked plugin
// confirm is never reached. Set it once for the whole file.
beforeAll(() => {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
});

describe("DictationScreen history load (A-3)", () => {
  it("shows the retry row, not the empty state, when list_dictation_history rejects", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_dictation_history") throw new Error("history file locked");
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<DictationScreen />);

    await waitFor(() => expect(screen.getByText(/could not be read/)).toBeTruthy());
    expect(screen.queryByText(/No dictations yet/)).toBeNull();
    expect(screen.getByText("Try again")).toBeTruthy();
  });

  it("shows the empty state when the read succeeds with zero rows", async () => {
    mockInvokeResponses({ list_dictation_history: [] });
    render(<DictationScreen />);

    await waitFor(() => expect(screen.getByText(/No dictations yet/)).toBeTruthy());
    expect(screen.queryByText(/could not be read/)).toBeNull();
    // An empty-but-successful read is not an error: no retry alert appears.
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("DictationScreen delete (A-1)", () => {
  it("keeps the row and shows the write error when delete_dictation_entry rejects", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_dictation_history") return [entry()];
      if (cmd === "delete_dictation_entry") throw new Error("disk full");
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    vi.mocked(confirm).mockResolvedValue(true);
    render(<DictationScreen />);

    await waitFor(() => expect(screen.getByText("Hello from a dictation")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("Delete entry"));

    // The row survives the failed delete and carries the error with a Retry.
    await waitFor(() => expect(screen.getByText(/did not save: disk full/)).toBeTruthy());
    expect(screen.getByText("Hello from a dictation")).toBeTruthy();
    expect(screen.getByText("Retry")).toBeTruthy();
    expect(invokeCalls()).toContainEqual({ cmd: "delete_dictation_entry", args: { id: "e1" } });
  });

  it("does not call the backend when the confirm dialog is declined", async () => {
    mockInvokeResponses({ list_dictation_history: [entry()] });
    vi.mocked(confirm).mockResolvedValue(false);
    render(<DictationScreen />);

    await waitFor(() => expect(screen.getByText("Hello from a dictation")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("Delete entry"));

    await waitFor(() => expect(vi.mocked(confirm)).toHaveBeenCalled());
    expect(invokeCalls().some((c) => c.cmd === "delete_dictation_entry")).toBe(false);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("DictationScreen copy (A-10)", () => {
  it("shows Copy failed, not Copied, when the clipboard write rejects", async () => {
    mockInvokeResponses({ list_dictation_history: [entry()] });
    const writeText = patchClipboard(false);
    render(<DictationScreen />);

    await waitFor(() => expect(screen.getByText("Hello from a dictation")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("Copy to clipboard"));

    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByTitle("Copy failed")).toBeTruthy());
    expect(screen.queryByTitle("Copied!")).toBeNull();
  });

  it("shows the Copied confirmation only when the write succeeds", async () => {
    mockInvokeResponses({ list_dictation_history: [entry()] });
    patchClipboard(true);
    render(<DictationScreen />);

    await waitFor(() => expect(screen.getByText("Hello from a dictation")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("Copy to clipboard"));

    await waitFor(() => expect(screen.getByTitle("Copied!")).toBeTruthy());
    expect(screen.queryByTitle("Copy failed")).toBeNull();
  });
});

describe("DictationScreen file queue (A-4 / A-11)", () => {
  it("queues three dropped files, processes one at a time, and names the position", async () => {
    mockInvokeResponses({ list_dictation_history: [] });
    render(<DictationScreen />);
    await waitFor(() => expect(screen.getByText(/Transcribe an audio file/)).toBeTruthy());

    // Gate each transcribe_file behind a manual promise so we can observe the
    // queue state between jobs: the old code overwrote `transcribe` per drop,
    // so three files read as one; the new code must say "2 of 3".
    const releases: Array<() => void> = [];
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "list_dictation_history") return [];
      if (cmd === "transcribe_file") {
        const i = releases.length;
        const path = (args as Record<string, unknown>)?.path;
        return new Promise((resolve) => {
          releases.push(() => resolve({ id: `t${i}`, wordCount: 10 + i, path }));
        });
      }
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    dropPaths(["/tmp/a.m4a", "/tmp/b.m4a", "/tmp/c.m4a"]);

    await waitFor(() =>
      expect(screen.getByText(/Transcribing 1 of 3, a\.m4a…/)).toBeTruthy()
    );
    // One file is in flight; the button gate has switched to Cancel.
    expect(invokeCalls().filter((c) => c.cmd === "transcribe_file")).toHaveLength(1);
    expect(screen.getByText("Cancel")).toBeTruthy();
    expect(screen.queryByText("Choose file")).toBeNull();

    // Finish the first file: the queue advances, not the whole batch.
    releases[0]();
    await waitFor(() =>
      expect(screen.getByText(/Transcribing 2 of 3, b\.m4a…/)).toBeTruthy()
    );

    // A drop while busy is gated: the button is gone, and the drag handler
    // ignores payloads while a job runs (A-11). The queue count is unchanged.
    dropPaths(["/tmp/extra.m4a"]);
    expect(screen.getByText(/Transcribing 2 of 3, b\.m4a…/)).toBeTruthy();

    // Finish the first two: the queue advances to the third, then we finish it.
    releases[1]();
    await waitFor(() =>
      expect(screen.getByText(/Transcribing 3 of 3, c\.m4a…/)).toBeTruthy()
    );
    releases[2]();
    await waitFor(() =>
      expect(screen.getByText("Transcribed 12 words")).toBeTruthy()
    );
    expect(
      invokeCalls().filter((c) => c.cmd === "list_dictation_history").length
    ).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("Choose file")).toBeTruthy();
  });
});
