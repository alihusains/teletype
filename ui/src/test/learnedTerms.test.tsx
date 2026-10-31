// claim: ui-behavior
/**
 * The Personalization tab's "What I learned" section (T12, UI half).
 *
 * Pins the agreed IPC contract (`list_learned_terms` / `undo_learned_term`) and
 * the behaviour the task requires:
 *   - the empty state copy,
 *   - one row per learned term: "Heard as <heardAs> → <writtenAs>", source chip,
 *     evidence count, last seen date, and the Undo button,
 *   - undo calls the backend with the term id and removes the row,
 *   - a failed undo restores the row and shows the inline error,
 *   - a failed list load renders the error state, not the empty state.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import PersonalizationScreen, { type LearnedTerm } from "../screens/PersonalizationScreen";
import { invokeCalls, mockInvokeResponses, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const DAY = 86_400_000;
const NOW = 1_760_000_000_000;

const EMPTY_PROFILE = {
  language: "en-US",
  preferences: [],
  learnFromEdits: true,
  learnAppSpecific: true,
  learnTerminology: true,
};

const term = (over: Partial<LearnedTerm>): LearnedTerm => ({
  id: 7,
  heardAs: ["Shavano"],
  writtenAs: ["Shivon"],
  source: "edit",
  evidence: 2,
  lastSeen: NOW - DAY,
  ...over,
});

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

describe("Personalization 'What I learned' section", () => {
  it("shows the empty state when nothing has been learned yet", async () => {
    mockInvokeResponses({ get_profile: EMPTY_PROFILE, list_learned_terms: [] });
    render(<PersonalizationScreen />);

    await waitFor(() =>
      expect(
        screen.getByText(
          "Nothing learned yet. Correct a word after dictating and Teletype will remember it."
        )
      ).toBeTruthy()
    );
  });

  it("renders one row per learned term with the heard→written pair and source chips", async () => {
    mockInvokeResponses({ get_profile: EMPTY_PROFILE, list_learned_terms: [term({})] });
    render(<PersonalizationScreen />);

    await waitFor(() => expect(screen.getByText("Heard as Shavano → Shivon")).toBeTruthy());

    expect(screen.getByText("From your edits")).toBeTruthy();
    expect(screen.getByText("2 corrections")).toBeTruthy();
    expect(screen.getByText(/Last seen/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Undo Shivon" })).toBeTruthy();
  });

  it("undoes the term: calls undo_learned_term with the id and removes the row", async () => {
    mockInvokeResponses({
      get_profile: EMPTY_PROFILE,
      list_learned_terms: [term({})],
      undo_learned_term: () => null,
    });
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText("Heard as Shavano → Shivon")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Undo Shivon" }));
    await waitFor(() =>
      expect(invokeCalls()).toContainEqual({ cmd: "undo_learned_term", args: { id: 7 } })
    );
    await waitFor(() => expect(screen.queryByText("Heard as Shavano → Shivon")).toBeNull());
  });

  it("restores a term whose undo failed and shows the inline error", async () => {
    mockInvokeResponses({
      get_profile: EMPTY_PROFILE,
      list_learned_terms: [term({})],
      undo_learned_term: () => Promise.reject(new Error("undo refused")),
    });
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText("Heard as Shavano → Shivon")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Undo Shivon" }));
    await waitFor(() => expect(screen.getByText(/did not save/)).toBeTruthy());
    // Optimistic undo rolled back: the row is back.
    expect(screen.getByText("Heard as Shavano → Shivon")).toBeTruthy();
  });

  it("shows the list-load error state instead of the empty state", async () => {
    mockInvokeResponses({
      get_profile: EMPTY_PROFILE,
      list_learned_terms: () => Promise.reject(new Error("locked")),
    });
    render(<PersonalizationScreen />);

    await waitFor(() =>
      expect(screen.getByText("Your learned terms could not be read.")).toBeTruthy()
    );
    expect(
      screen.queryByText(
        "Nothing learned yet. Correct a word after dictating and Teletype will remember it."
      )
    ).toBeNull();
  });
});
