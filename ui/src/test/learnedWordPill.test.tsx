// claim: ui-behavior
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import {
  emitTauriEvent,
  invokeCalls,
  listenHandlersFor,
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
} from "./tauriMock";
import {
  clampLearnUndoSecs,
  learnResumeElapsedMs,
  pillReducer,
  initialPillMachineState,
} from "../pillMachine";
import { LEARNED_COPY } from "../pill";

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
vi.stubGlobal("matchMedia", (media: string) => ({
  media,
  matches: false,
  addEventListener() {},
  removeEventListener() {},
  addListener() {},
  removeListener() {},
}));
vi.stubGlobal("requestAnimationFrame", () => 1);
vi.stubGlobal("cancelAnimationFrame", () => {});
// jsdom has no layout: give measured branches a nonzero box so the card id
// reaches set_pill_size, which is what the hidden overlay waits for.
vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function () {
  return {
    x: 0, y: 0, width: 320, height: 52, top: 0, left: 0, right: 320, bottom: 52,
    toJSON: () => ({}),
  } as DOMRect;
});

async function mount(learnUndoSecs = 12): Promise<void> {
  mockInvokeResponses({
    get_settings: {
      recordingMode: "hold",
      language: "en",
      inputDevice: "",
      pillStyle: "default",
      learnUndoSecs,
    },
    list_input_devices: [],
    set_pill_size: undefined,
    dismiss_pill_result: undefined,
    undo_learned: "applied",
  });
  mockListenCapture();
  document.body.innerHTML = '<div id="root"></div>';
  vi.resetModules();
  await import("../pillEntry");
  await waitFor(() => {
    expect(listenHandlersFor("learned-word")).toHaveLength(1);
  });
  await new Promise((r) => setTimeout(r, 0));
}

async function emit(payload: Record<string, unknown>): Promise<void> {
  await act(async () => {
    await emitTauriEvent("learned-word", payload);
  });
}

function sizeForCard(card: number): Record<string, unknown> | undefined {
  const calls = invokeCalls().filter(
    (c) => c.cmd === "set_pill_size" && (c.args as { card?: number } | undefined)?.card === card,
  );
  return calls[calls.length - 1]?.args as Record<string, unknown> | undefined;
}

describe("learned-pill timing helpers (pure)", () => {
  it("defaults to 12 s and clamps to 3..300", () => {
    expect(clampLearnUndoSecs(undefined)).toBe(12);
    expect(clampLearnUndoSecs(Number.NaN)).toBe(12);
    expect(clampLearnUndoSecs(0)).toBe(3);
    expect(clampLearnUndoSecs(12)).toBe(12);
    expect(clampLearnUndoSecs(9999)).toBe(300);
  });

  it("resumes after hover with at least a two-second floor", () => {
    // 11.5 s of a 12 s dwell leaves 0.5 s; resume rewinds to 2 s left.
    expect(learnResumeElapsedMs(12_000, 11_500)).toBe(10_000);
    // Plenty of time left: elapsed is unchanged.
    expect(learnResumeElapsedMs(12_000, 4_000)).toBe(4_000);
  });

  it("drops stale card ids and maps undo results without reviving a new take", () => {
    let s = initialPillMachineState();
    s = pillReducer(s, {
      type: "learned-show",
      payload: { card: 7, offerId: 42, word: "Markets", kind: "added" },
    });
    expect(s.pill.phase).toBe("learned");
    // An older card arriving late is ignored.
    s = pillReducer(s, {
      type: "learned-show",
      payload: { card: 6, offerId: 41, word: "Old", kind: "added" },
    });
    expect(s.pill.phase === "learned" && s.pill.word).toBe("Markets");
    s = pillReducer(s, { type: "learned-undo", card: 7 });
    expect(s.pill.phase === "learned" && s.pill.status).toBe("pending");
    s = pillReducer(s, { type: "learned-undo-result", card: 7, result: "applied" });
    expect(s.pill.phase === "learned" && s.pill.status).toBe("undone");
    s = pillReducer(s, { type: "learned-undo-result", card: 7, result: "stale" });
    expect(s.pill.phase === "learned" && s.pill.status).toBe("undone");
    // A new take replaces a learned pill; a late dismiss cannot resurrect it.
    s = pillReducer(s, {
      type: "pill-state",
      payload: { phase: "recording", startedAtMs: 1, command: false },
      now: 1,
    });
    s = pillReducer(s, { type: "learned-dismiss", card: 7 });
    expect(s.pill.phase).toBe("recording");
  });
});

describe("learned-word pill UI", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    resetTauriMocks();
  });

  it("shows the canonical word, exact subtitle, a11y labels and Undo/×, and reports the card id", async () => {
    await mount();
    vi.useFakeTimers();
    await emit({ card: 7, offerId: 42, word: "Markets", kind: "added" });

    expect(screen.getByText("Markets")).toBeTruthy();
    expect(screen.getByText(LEARNED_COPY.added)).toBeTruthy();
    expect(
      screen.getByLabelText("Markets, Added to dictionary. Undo available."),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Undo" })).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Close and keep “Markets”" }),
    ).toBeTruthy();
    // The size report carries the card id so overlay::show_learned can show
    // the hidden window for this card and no other.
    expect(sizeForCard(7)?.card).toBe(7);
  });

  it("shrinks a long word to the 7.5 px floor before truncating", async () => {
    await mount();
    const longWord = "Supercalifragilisticexpialidocious".repeat(4);
    await emit({ card: 15, offerId: 49, word: longWord, kind: "added" });
    expect(screen.getByText(longWord).style.fontSize).toBe("7.5px");
  });

  it("uses the updated subtitle for an existing word", async () => {
    await mount();
    vi.useFakeTimers();
    await emit({ card: 8, offerId: 43, word: "Shivon", kind: "updated" });
    expect(screen.getByText(LEARNED_COPY.updated)).toBeTruthy();
  });

  it("expires after the configured 12 s dwell", async () => {
    await mount(12);
    vi.useFakeTimers();
    await emit({ card: 9, offerId: 44, word: "Markets", kind: "added" });
    expect(screen.getByTestId("learned-pill")).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(11_900);
    });
    expect(screen.getByTestId("learned-pill")).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(200);
    });
    expect(screen.queryByTestId("learned-pill")).toBeNull();
    // Leaving idle hands the card back to Rust.
    expect(
      invokeCalls().some(
        (c) => c.cmd === "dismiss_pill_result" && (c.args as { card: number }).card === 9,
      ),
    ).toBe(true);
  });

  it("pauses on hover and resumes with at least two seconds left", async () => {
    await mount(12);
    vi.useFakeTimers();
    await emit({ card: 10, offerId: 45, word: "Markets", kind: "added" });
    await act(async () => {
      vi.advanceTimersByTime(11_500);
    });
    expect(screen.queryByTestId("learned-pill")).toBeTruthy();
    fireEvent.mouseEnter(screen.getByTestId("learned-pill"));
    await act(async () => {
      vi.advanceTimersByTime(5_000);
    });
    expect(screen.queryByTestId("learned-pill")).toBeTruthy();
    fireEvent.mouseLeave(screen.getByTestId("learned-pill"));
    await act(async () => {
      vi.advanceTimersByTime(1_900);
    });
    expect(screen.queryByTestId("learned-pill")).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(200);
    });
    expect(screen.queryByTestId("learned-pill")).toBeNull();
  });

  it("morphs through pending to undone, and shows the undo-error copy on stale", async () => {
    await mount();
    vi.useFakeTimers();
    await emit({ card: 11, offerId: 46, word: "Markets", kind: "added" });
    await act(async () => {
      screen.getByRole("button", { name: "Undo" }).click();
    });
    // Pending reserves the controls' space: the Undo button stays mounted
    // but disabled/hidden, so the pill does not resize mid-undo.
    expect(screen.queryByTestId("learned-pill")).toBeTruthy();
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByTestId("learned-result").textContent).toBe(LEARNED_COPY.removed("Markets"));
    expect(invokeCalls().some((c) => c.cmd === "undo_learned")).toBe(true);

    // A stale claim produces the error copy for a different card.
    await emit({ card: 12, offerId: 47, word: "Shivon", kind: "updated" });
    mockInvokeResponses({
      get_settings: { recordingMode: "hold", language: "en", inputDevice: "", pillStyle: "default", learnUndoSecs: 12 },
      list_input_devices: [],
      set_pill_size: undefined,
      dismiss_pill_result: undefined,
      undo_learned: "stale",
    });
    await act(async () => {
      screen.getByRole("button", { name: "Undo" }).click();
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByTestId("learned-result").textContent).toBe(LEARNED_COPY.undoError);
  });

  it("× keeps the word without calling undo", async () => {
    await mount();
    vi.useFakeTimers();
    await emit({ card: 13, offerId: 48, word: "Markets", kind: "added" });
    fireEvent.click(screen.getByRole("button", { name: "Close and keep “Markets”" }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.queryByTestId("learned-pill")).toBeNull();
    expect(invokeCalls().some((c) => c.cmd === "undo_learned")).toBe(false);
  });

  it("renders the save-failure notice with no Undo button", async () => {
    await mount();
    vi.useFakeTimers();
    await act(async () => {
      await emitTauriEvent("learned-word-error", { card: 14, word: "Markets" });
    });
    expect(screen.getByTestId("learned-error-pill").textContent).toBe(LEARNED_COPY.saveError("Markets"));
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
    expect(sizeForCard(14)?.card).toBe(14);
  });
});
