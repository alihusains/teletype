// claim: ui-behavior
/**
 * The Home screen's independent loads (audit docs/app-states-audit.md, A-3):
 *
 * The old `refresh` raced two `invoke` calls behind one `.catch(() => {})`
 * and a shared state pair, so a rejected `get_insights` could leave the
 * recent-dictations list blank without saying why. Now each read is an
 * independent resource: a failure renders a LoadFailed row in its own slot
 * with its own retry, and the other column keeps working.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import HomeScreen from "../screens/HomeScreen";
import { mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onDragDropEvent: () => Promise.resolve(() => {}),
    onFocusChanged: () => Promise.resolve(() => {}),
  }),
}));

const HISTORY = [
  {
    id: "h1",
    createdAt: Date.now(),
    text: "Meeting notes from this morning",
    appName: "Mail",
    appType: "Email",
  },
];

/** A plausible get_insights payload matching HomeScreen's Insights interface. */
const INSIGHTS = {
  totalWords: 1000,
  totalDictations: 20,
  streakDays: 3,
  impact: {
    wordsPerMinute: 150,
    timesFaster: 2.1,
    timeSavedLabel: "2h 15m",
    timeSavedMinutes: 135,
    ratedTakes: 5,
  },
  wordsLast7Days: 400,
  avgWordsPerDay: 100,
};

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

describe("HomeScreen independent loads (A-3)", () => {
  it("shows a retry row for insights while the recent list still renders when get_insights rejects", async () => {
    mockInvokeResponses({ list_dictation_history: HISTORY });
    mockListenCapture();
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_dictation_history") return HISTORY;
      if (cmd === "get_insights") throw new Error("insights db locked");
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<HomeScreen />);

    // The history column is intact…
    await waitFor(() =>
      expect(screen.getByText("Meeting notes from this morning")).toBeTruthy()
    );
    // …and the failed slot says exactly what failed, with its own retry.
    await waitFor(() => expect(screen.getByText(/Your insights/)).toBeTruthy());
    expect(screen.getByText(/could not be read/)).toBeTruthy();
    expect(screen.getByText(/insights db locked/)).toBeTruthy();
    expect(screen.getByText("Try again")).toBeTruthy();
  });

  it("shows a retry row for recent dictations while insights still renders when list_dictation_history rejects", async () => {
    mockInvokeResponses({ get_insights: INSIGHTS });
    mockListenCapture();
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_dictation_history") throw new Error("history file locked");
      if (cmd === "get_insights") return INSIGHTS;
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<HomeScreen />);

    // The insights card is intact…
    await waitFor(() => expect(screen.getByText("2h 15m")).toBeTruthy());
    // …and the failed slot has its own retry, independent of the other one.
    await waitFor(() => expect(screen.getByText(/Recent dictations/)).toBeTruthy());
    expect(screen.getByText(/history file locked/)).toBeTruthy();
    expect(screen.getByText("Try again")).toBeTruthy();
  });

  it("shows both retry rows when both reads reject", async () => {
    mockListenCapture();
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_dictation_history") throw new Error("history locked");
      if (cmd === "get_insights") throw new Error("insights locked");
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<HomeScreen />);

    await waitFor(() => expect(screen.getByText(/Recent dictations/)).toBeTruthy());
    expect(screen.getByText(/Your insights/)).toBeTruthy();
    // Two distinct details, both visible at once.
    expect(screen.getByText(/history locked/)).toBeTruthy();
    expect(screen.getByText(/insights locked/)).toBeTruthy();
  });

  it("renders neither retry row when both reads succeed", async () => {
    mockInvokeResponses({ list_dictation_history: HISTORY, get_insights: INSIGHTS });
    mockListenCapture();
    render(<HomeScreen />);

    await waitFor(() => expect(screen.getByText("Meeting notes from this morning")).toBeTruthy());
    expect(screen.getByText("2h 15m")).toBeTruthy();
    // Both resources settled successfully: no retry row remains.
    await waitFor(() => expect(screen.queryByText(/could not be read/)).toBeNull());
  });
});
