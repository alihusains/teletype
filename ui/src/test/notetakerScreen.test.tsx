// claim: ui-behavior
/**
 * The Notetaker screen replaces the old append-only Scratchpad with real
 * notes: a list pane (search, create, delete) and an editor pane (title,
 * body) with debounced autosave. The behaviour under test:
 *
 *   1. Notes load from `list_notes` and the open note from `get_open_note`.
 *   2. Selecting a note calls `open_note` and loads it into the editor.
 *   3. Typing marks the note dirty and autosaves via `save_note` after the
 *      debounce (vitest fake timers make this deterministic).
 *   4. Delete asks for confirmation first and only then calls `delete_note`.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import NotetakerScreen from "../screens/NotetakerScreen";
import { invoke } from "@tauri-apps/api/core";
import { mockInvokeResponses, invokeCalls, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const NOTE_A = {
  id: "n1",
  title: "Meeting notes",
  body: "First body line.\nSecond line.",
  preview: "First body line.",
  createdAt: 1750000000000,
  updatedAt: 1750001000000,
  source: "typed" as const,
};

const NOTE_B = {
  ...NOTE_A,
  id: "n2",
  title: "Groceries",
  body: "Milk and eggs",
  preview: "Milk and eggs",
  updatedAt: 1750002000000,
  source: "dictated" as const,
};

beforeEach(() => {
  const echoSave = (id: string, title: string, body: string) => ({
    ...NOTE_A,
    id,
    title,
    body,
  });
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === "save_note") {
      const a = args as { id: string; title: string; body: string };
      return echoSave(a.id, a.title, a.body);
    }
    throw new Error(`[tauriMock] unexpected invoke: ${cmd}`);
  });
  mockInvokeResponses({
    list_notes: [NOTE_B, NOTE_A],
    get_open_note: NOTE_A,
    get_settings: { scratchpadEnabled: false },
    open_note: null,
    save_note: NOTE_A,
    create_note: null,
    delete_note: null,
    save_settings: {},
  });
});

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

describe("NotetakerScreen", () => {
  it("lists notes and loads the open one into the editor", async () => {
    render(<NotetakerScreen />);
    await waitFor(() =>
      expect(
        (screen.getByRole("textbox", { name: /note body/i }) as HTMLTextAreaElement)
          .value
      ).toBe(NOTE_A.body)
    );
    // Both titles are visible in the list pane.
    expect(screen.getByText("Meeting notes")).toBeTruthy();
    expect(screen.getByText("Groceries")).toBeTruthy();
  });

  it("selecting a note calls open_note and loads its contents", async () => {
    render(<NotetakerScreen />);
    await screen.findByText("Groceries");
    fireEvent.click(screen.getByText("Groceries"));
    await waitFor(() =>
      expect(
        (screen.getByRole("textbox", { name: /note body/i }) as HTMLTextAreaElement)
          .value
      ).toBe(NOTE_B.body)
    );
    expect(invokeCalls()).toContainEqual({
      cmd: "open_note",
      args: { id: "n2" },
    });
  });

  it("autosaves edited text through save_note after the debounce", async () => {
    render(<NotetakerScreen />);
    const body = await screen.findByRole("textbox", { name: /note body/i });
    fireEvent.change(body, { target: { value: "Edited body." } });
    // Not yet saved inside the debounce window.
    expect(invokeCalls().some((c) => c.cmd === "save_note")).toBe(false);
    // The screen debounces saves at 600ms. Let real timers elapse, then drain
    // the microtask queue so the pending invoke resolves.
    await new Promise((r) => setTimeout(r, 750));
    await waitFor(() =>
      expect(invokeCalls().some((c) => c.cmd === "save_note")).toBe(true)
    );
    const save = invokeCalls().find((c) => c.cmd === "save_note");
    expect(save?.args).toMatchObject({
      id: "n1",
      title: "Meeting notes",
      body: "Edited body.",
    });
  });

  it("delete asks for confirmation before calling delete_note", async () => {
    const confirm = vi
      .spyOn(window, "confirm")
      .mockReturnValue(true);
    render(<NotetakerScreen />);
    await screen.findByText("Groceries");
    const row = screen.getByText("Groceries").closest("[role='button']")!;
    fireEvent.click(within(row as HTMLElement).getByLabelText("Delete note"));
    await waitFor(() =>
      expect(invokeCalls().some((c) => c.cmd === "delete_note")).toBe(true)
    );
    expect(confirm).toHaveBeenCalled();
    confirm.mockRestore();
  });

  it("search filters the list without a backend call", async () => {
    render(<NotetakerScreen />);
    await screen.findByText("Groceries");
    fireEvent.change(screen.getByPlaceholderText(/search notes/i), {
      target: { value: "milk" },
    });
    expect(screen.getByText("Groceries")).toBeTruthy();
    expect(screen.queryByText("Meeting notes")).toBeNull();
  });
});
