// claim: ui-behavior
/**
 * The Dictation tab's detail card (docs/dictation-detail-spec.md section 5):
 * collapsed meta row, expand by click / Enter / Space, inline stage edits,
 * the recording player, the actions, search, and accessibility.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { axe } from "vitest-axe";
import DictationScreen from "../screens/DictationScreen";
import { clearAudioCache } from "../components/dictation/AudioPlayer";
import { SHOW_CHANGES_KEY } from "../components/dictation/DictationCard";
import {
  barsForWidth,
  makeWav,
  peaksFromSamples,
  parseWav,
  resamplePeaks,
  waveformFromWav,
} from "../components/dictation/waveform";
import type { HistoryEntry } from "../components/dictation/types";
import { invokeCalls, mockInvokeResponses, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
  message: vi.fn(),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onDragDropEvent: () => Promise.resolve(() => {}),
    onFocusChanged: () => Promise.resolve(() => {}),
  }),
}));

const AXE_RULES = { rules: { "color-contrast": { enabled: false } } };

function entry(over: Partial<HistoryEntry> = {}): HistoryEntry {
  return {
    id: "e1",
    createdAt: new Date(2026, 9, 31, 23, 39).getTime(),
    text: "Hey sam quick status the",
    appName: "Terax",
    appType: "Terminal",
    rawText: "hey sam um quick update the",
    edits: [
      { kind: "polish", from: "hey", to: "Hey", minor: true },
      { kind: "same", from: "sam" },
      { kind: "filler", from: "um" },
      { kind: "same", from: "quick" },
      { kind: "polish", from: "update", to: "status" },
      { kind: "same", from: "the" },
    ],
    asrModel: "Parakeet v3",
    polishModel: "Qwen3.5 2B",
    polishMs: 400,
    durationMs: 2000,
    wordCount: 3,
    hasRecording: true,
    ...over,
  };
}

/** A 2 s synthetic take: one second of tone, one of silence. */
function takeWav(): ArrayBuffer {
  const s = new Float32Array(32000);
  for (let i = 0; i < 16000; i++) s[i] = 0.5 * Math.sin(i / 5);
  return makeWav(s);
}

async function renderWith(rows: HistoryEntry[], extra: Record<string, unknown> = {}) {
  mockInvokeResponses({ list_dictation_history: rows, get_dictation_audio: takeWav(), ...extra });
  const view = render(<DictationScreen />);
  await waitFor(() => expect(screen.getAllByRole("button", { expanded: false }).length).toBeGreaterThan(0));
  return view;
}

function header() {
  return screen.getAllByRole("button", { expanded: false })[0] ?? screen.getAllByRole("button", { expanded: true })[0];
}

beforeAll(() => {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
});

beforeEach(() => {
  clearAudioCache();
  try {
    localStorage.removeItem(SHOW_CHANGES_KEY);
  } catch {
    // jsdom always has storage; the guard mirrors the component.
  }
  vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: vi.fn(() => "blob:take"), revokeObjectURL: vi.fn() }));
  vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(function (this: HTMLMediaElement) {
    this.dispatchEvent(new Event("play"));
    return Promise.resolve();
  });
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(function (this: HTMLMediaElement) {
    this.dispatchEvent(new Event("pause"));
  });
});

afterEach(() => {
  cleanup();
  resetTauriMocks();
  vi.mocked(confirm).mockReset();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("collapsed card", () => {
  it("shows the first line, words, duration and app chip, and nothing else", async () => {
    await renderWith([entry()]);
    const head = header();
    expect(head.getAttribute("aria-expanded")).toBe("false");
    expect(within(head).getByText("Hey sam quick status the")).toBeTruthy();
    expect(within(head).getByText("3 words")).toBeTruthy();
    expect(within(head).getByText("2s")).toBeTruthy();
    expect(within(head).getByText("Terax")).toBeTruthy();
    expect(within(head).getByText("11:39 PM")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Play recording/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
    // Exactly one copy control on the collapsed card: the small icon next
    // to the chevron. The expanded Copy button is not present.
    expect(screen.getAllByRole("button", { name: /Copy/i })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Copy dictation" })).toBeTruthy();
    // No audio is fetched until a card is opened.
    expect(invokeCalls().some((c) => c.cmd === "get_dictation_audio")).toBe(false);
  });

  it("says 1 word and hides the duration when it is unknown", async () => {
    await renderWith([entry({ wordCount: 1, durationMs: null, text: "Hi" })]);
    expect(screen.getByText("1 word")).toBeTruthy();
    expect(screen.queryByText("2s")).toBeNull();
  });

  it("copies from the small icon without toggling the card", async () => {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      configurable: true,
    });
    await renderWith([entry({ hasRecording: false })]);
    const head = header();
    fireEvent.click(screen.getByRole("button", { name: "Copy dictation" }));
    // The card stays collapsed: the icon is a sibling of the header, so a
    // click on it never reaches the header's toggle.
    expect(head.getAttribute("aria-expanded")).toBe("false");
    // Same success feedback as the expanded Copy: "Copied" for 1.2 s.
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Copy dictation" }).getAttribute("title"),
      ).toBe("Copied!")
    );
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("Hey sam quick status the");
  });

  it("shows the copy failure on the small icon", async () => {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
      configurable: true,
    });
    await renderWith([entry({ hasRecording: false })]);
    fireEvent.click(screen.getByRole("button", { name: "Copy dictation" }));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Copy dictation" }).getAttribute("title"),
      ).toBe("Copy failed")
    );
  });
});

describe("expanding", () => {
  it("toggles with a click and reveals the text, actions and model", async () => {
    await renderWith([entry({ hasRecording: false })]);
    fireEvent.click(header());
    const head = screen.getByRole("button", { expanded: true });
    const region = screen.getByRole("region");
    expect(head.getAttribute("aria-controls")).toBe(region.id);
    expect(screen.getByRole("button", { name: "Copy" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Delete" })).toBeTruthy();
    // Exactly one copy control on the expanded card: the Copy button. The
    // small collapsed-card icon is not rendered (not even focusable).
    expect(screen.getAllByRole("button", { name: /Copy/i })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Copy dictation" })).toBeNull();
    expect(screen.getByText("Parakeet v3 · Polished by Qwen3.5 2B")).toBeTruthy();
    fireEvent.click(head);
    expect(screen.queryByRole("region")).toBeNull();
  });

  it("toggles with Enter and Space (native button keys)", async () => {
    await renderWith([entry({ hasRecording: false })]);
    const head = header();
    // A native <button> turns Enter/Space into a click; jsdom does not, so
    // assert it is a real button and drive the click it would produce.
    expect(head.tagName).toBe("BUTTON");
    expect(head.getAttribute("type")).toBe("button");
    fireEvent.keyDown(head, { key: "Enter" });
    fireEvent.click(head);
    expect(head.getAttribute("aria-expanded")).toBe("true");
    fireEvent.keyDown(head, { key: " " });
    fireEvent.click(head);
    expect(head.getAttribute("aria-expanded")).toBe("false");
  });
});

describe("inline edits", () => {
  it("strikes the filler, marks the polish replacement and leaves case fixes plain", async () => {
    const { container } = await renderWith([entry({ hasRecording: false })]);
    fireEvent.click(header());
    const dels = Array.from(container.querySelectorAll("del")).map((d) => d.textContent);
    const inss = Array.from(container.querySelectorAll("ins")).map((d) => d.textContent);
    expect(dels).toEqual(["um", "update"]);
    expect(inss).toEqual(["status"]);
    // The minor span renders only its new word, undecorated.
    expect(container.querySelector('[data-kind="polish"] del')?.textContent).toBe("update");
    expect(screen.getByRole("region").textContent).toMatch(/^Hey sam/);
    expect(screen.getByTitle("Filler removed: um")).toBeTruthy();
    expect(screen.getByTitle("Polish changed update to status")).toBeTruthy();
    expect(screen.getByText("1 filler removed · 1 polish edit · polished in 0.4s")).toBeTruthy();
  });

  it("drops every del and ins when Show changes is off, and remembers it", async () => {
    const { container } = await renderWith([entry({ hasRecording: false })]);
    fireEvent.click(header());
    fireEvent.click(screen.getByRole("switch", { name: "Show changes" }));
    expect(container.querySelectorAll("del, ins")).toHaveLength(0);
    expect(screen.getByRole("region").textContent).toContain("Hey sam quick status the");
    expect(localStorage.getItem(SHOW_CHANGES_KEY)).toBe("0");
  });

  it("renders plain text and no summary when edits are null", async () => {
    const { container } = await renderWith([entry({ edits: null, hasRecording: false })]);
    fireEvent.click(header());
    expect(container.querySelectorAll("del, ins")).toHaveLength(0);
    expect(screen.queryByRole("switch", { name: "Show changes" })).toBeNull();
  });
});

describe("inline edits, review fixes", () => {
  it("keeps the pasted text's line breaks with Show changes on", async () => {
    const text = "Hi Sam,\n\nThanks for the notes.\n\nBest,";
    const { container } = await renderWith([
      entry({
        hasRecording: false,
        text,
        edits: [
          { kind: "same", from: "Hi Sam," },
          { kind: "filler", from: "um" },
          { kind: "same", from: "Thanks for the notes. Best," },
        ],
      }),
    ]);
    fireEvent.click(header());
    expect(container.querySelectorAll("del")).toHaveLength(1);
    // Exactly the pasted text once the struck filler and its space go.
    const region = screen.getByRole("region").firstElementChild as HTMLElement;
    const shown = region.textContent!.replace("Filler removed: um. ", "").replace(" um", "");
    expect(shown).toBe(text);
  });

  it("falls back to the plain text when the edits do not spell it", async () => {
    const { container } = await renderWith([
      entry({ hasRecording: false, text: "Something else was pasted" }),
    ]);
    fireEvent.click(header());
    expect(container.querySelectorAll("del, ins")).toHaveLength(0);
    expect(screen.getByRole("region").textContent).toContain("Something else was pasted");
  });

  it("keeps screen-reader labels and struck words out of a copied selection", async () => {
    const { container } = await renderWith([entry({ hasRecording: false })]);
    fireEvent.click(header());
    const del = container.querySelector("del") as HTMLElement;
    expect(del.style.userSelect).toBe("none");
    const label = within(container.querySelector('[data-kind="filler"]') as HTMLElement).getByText(
      "Filler removed: um."
    );
    expect(label.style.userSelect).toBe("none");
  });

  it("credits both stages when a later one rewrote an earlier change", async () => {
    const { container } = await renderWith([
      entry({
        hasRecording: false,
        text: "call Jonathan tomorrow",
        edits: [
          { kind: "same", from: "call" },
          { kind: "dictionary", from: "jon", to: "John", superseded: true },
          { kind: "polish", from: "John", to: "Jonathan" },
          { kind: "same", from: "tomorrow" },
        ],
      }),
    ]);
    fireEvent.click(header());
    expect(Array.from(container.querySelectorAll("del")).map((d) => d.textContent)).toEqual(["jon", "John"]);
    expect(Array.from(container.querySelectorAll("ins")).map((d) => d.textContent)).toEqual(["Jonathan"]);
    expect(screen.getByTitle("Dictionary fixed jon to John, then changed again")).toBeTruthy();
    expect(screen.getByText("1 polish edit · 1 dictionary fix · polished in 0.4s")).toBeTruthy();
  });

  it("toggles Show changes from its visible words too", async () => {
    const { container } = await renderWith([entry({ hasRecording: false })]);
    fireEvent.click(header());
    fireEvent.click(screen.getByText("Show changes"));
    expect(screen.getByRole("switch", { name: "Show changes" }).getAttribute("aria-checked")).toBe("false");
    expect(container.querySelectorAll("del, ins")).toHaveLength(0);
  });

  it("sizes Copy for its longest label so the buttons after it never move", async () => {
    await renderWith([entry({ hasRecording: false })]);
    fireEvent.click(header());
    const copy = screen.getByRole("button", { name: "Copy" });
    const labels = Array.from(copy.querySelectorAll("span[style*='grid-area']")).map((s) => s.textContent);
    expect(labels).toEqual(["Copy", "Copied", "Copy failed"]);
  });
});

describe("long take (segment-polished)", () => {
  // The 24 s take polished segment by segment: the per-segment polish
  // removed a filler and rewrote a phrase, so the inline diff must show
  // both, exactly like a short take.
  function longTake(): HistoryEntry {
    return entry({
      id: "long1",
      text:
        "Quick update on the staging rollout. The schema check timed out twice this morning, so we pinned the older client. The platform team ships the fix on Friday, and we revisit the rollout plan on Monday before standup.",
      rawText:
        "quick update on the staging rollout um the schema check timed out twice this morning so we pinned the older client the platform team ships the fix on friday and we revisit the rollout plan on monday before standup",
      edits: [
        { kind: "polish", from: "quick", to: "Quick", minor: true },
        { kind: "same", from: "update on the staging rollout." },
        { kind: "filler", from: "um" },
        {
          kind: "polish",
          from: "the schema check timed out twice this morning so we pinned the older client",
          to: "The schema check timed out twice this morning, so we pinned the older client.",
        },
        { kind: "same", from: "The platform team ships the fix on" },
        { kind: "polish", from: "friday", to: "Friday,", minor: true },
        { kind: "same", from: "and we revisit the rollout plan on" },
        { kind: "polish", from: "monday", to: "Monday", minor: true },
        { kind: "polish", from: "before standup", to: "before standup.", minor: true },
      ],
      polishMs: 1900,
      durationMs: 24000,
      wordCount: 38,
      hasRecording: true,
    });
  }

  it("shows the struck filler and the polish replacement with Show changes on", async () => {
    const { container } = await renderWith([longTake()]);
    fireEvent.click(header());
    const region = screen.getByRole("region");
    // The filler is struck through.
    const del = Array.from(container.querySelectorAll("del")).find((d) => d.textContent === "um");
    expect(del).toBeTruthy();
    expect(del!.style.textDecoration).toBe("line-through");
    expect(screen.getByTitle("Filler removed: um")).toBeTruthy();
    // The phrase rewrite shows the old words struck and the new words
    // highlighted.
    expect(
      Array.from(container.querySelectorAll("del")).map((d) => d.textContent).join("|"),
    ).toContain("the schema check timed out twice this morning so we pinned the older client");
    expect(
      Array.from(container.querySelectorAll("ins")).map((d) => d.textContent).join("|"),
    ).toContain("The schema check timed out twice this morning, so we pinned the older client.");
    // The summary agrees with what is shown.
    expect(screen.getByText("1 filler removed · 1 polish edit · polished in 1.9s")).toBeTruthy();
    // The final text reads correctly once the struck words go.
    expect(region.textContent!.includes("Quick update on the staging rollout.")).toBe(true);
  });

  it("shows only the final text with Show changes off", async () => {
    const { container } = await renderWith([longTake()]);
    fireEvent.click(header());
    fireEvent.click(screen.getByRole("switch", { name: "Show changes" }));
    expect(container.querySelectorAll("del, ins")).toHaveLength(0);
    expect(screen.getByRole("region").textContent).toContain(
      "Quick update on the staging rollout. The schema check timed out twice this morning, so we pinned the older client."
    );
  });
});

describe("legacy and no-audio rows", () => {
  it("renders a legacy row with no player, no Show in Finder and no model line", async () => {
    const errors = vi.spyOn(console, "error");
    await renderWith([
      entry({
        rawText: null,
        edits: null,
        asrModel: null,
        polishModel: null,
        polishMs: null,
        durationMs: null,
        hasRecording: false,
      }),
    ]);
    fireEvent.click(header());
    expect(screen.queryByRole("button", { name: /Play recording/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Show in/ })).toBeNull();
    expect(screen.queryByText(/Parakeet/)).toBeNull();
    expect(errors).not.toHaveBeenCalled();
  });
});

describe("audio player", () => {
  it("fetches on expand, draws 96 bars, plays, and seeks with the keyboard", async () => {
    await renderWith([entry()]);
    fireEvent.click(header());
    const play = await screen.findByRole("button", { name: "Play recording" });
    await waitFor(() => expect(play.hasAttribute("disabled")).toBe(false));
    expect(invokeCalls()).toContainEqual({ cmd: "get_dictation_audio", args: { id: "e1" } });

    const slider = screen.getByRole("slider", { name: "Seek" });
    expect(slider.querySelectorAll("[data-bar]")).toHaveLength(96);
    expect(slider.getAttribute("aria-valuetext")).toBe("0:00 of 0:02");
    expect(screen.getByText("0:00 / 0:02")).toBeTruthy();

    await act(async () => {
      fireEvent.click(play);
    });
    expect(screen.getByRole("button", { name: "Pause recording" })).toBeTruthy();

    fireEvent.keyDown(slider, { key: "ArrowRight" });
    expect(slider.getAttribute("aria-valuenow")).toBe("1");
    const audio = document.querySelector("audio") as HTMLAudioElement;
    expect(audio.currentTime).toBe(1);
    fireEvent.keyDown(slider, { key: "End" });
    expect(slider.getAttribute("aria-valuetext")).toBe("0:02 of 0:02");
    fireEvent.keyDown(slider, { key: "Home" });
    expect(slider.getAttribute("aria-valuenow")).toBe("0");
  });

  it("shows a one-line error when the recording cannot be loaded", async () => {
    mockInvokeResponses({
      list_dictation_history: [entry()],
      get_dictation_audio: () => Promise.reject(new Error("gone")),
    });
    render(<DictationScreen />);
    await waitFor(() => expect(header()).toBeTruthy());
    fireEvent.click(header());
    expect(await screen.findByText("Could not load this recording.")).toBeTruthy();
  });

  it("Show in Finder reveals the recording by id, Explorer on Windows", async () => {
    await renderWith([entry()], { reveal_dictation_recording: null });
    fireEvent.click(header());
    fireEvent.click(screen.getByRole("button", { name: "Show in Finder" }));
    expect(invokeCalls()).toContainEqual({ cmd: "reveal_dictation_recording", args: { id: "e1" } });
    cleanup();

    vi.stubGlobal("navigator", { ...navigator, userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)" });
    await renderWith([entry()], { reveal_dictation_recording: null });
    fireEvent.click(header());
    expect(screen.getByRole("button", { name: "Show in Explorer" })).toBeTruthy();
  });
});

describe("delete", () => {
  it("still confirms and calls delete_dictation_entry", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    await renderWith([entry({ hasRecording: false })], { delete_dictation_entry: null });
    fireEvent.click(header());
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(invokeCalls()).toContainEqual({ cmd: "delete_dictation_entry", args: { id: "e1" } })
    );
    expect(vi.mocked(confirm)).toHaveBeenCalled();
  });
});

describe("search", () => {
  it("matches the raw transcript and the app name, not just the final text", async () => {
    await renderWith([
      entry({ id: "a", text: "Polished words", rawText: "um polished words", appName: "Terax" }),
      entry({ id: "b", text: "Other take", rawText: "other take", appName: "Mail" }),
    ]);
    const search = screen.getByPlaceholderText("Search…");
    fireEvent.change(search, { target: { value: "um polished" } });
    expect(screen.getByText("Polished words")).toBeTruthy();
    expect(screen.queryByText("Other take")).toBeNull();
    fireEvent.change(search, { target: { value: "mail" } });
    expect(screen.getByText("Other take")).toBeTruthy();
    expect(screen.queryByText("Polished words")).toBeNull();
  });
});

describe("accessibility", () => {
  it("has no axe violations collapsed or expanded", async () => {
    const { container } = await renderWith([entry({ hasRecording: false })]);
    expect((await axe(container, AXE_RULES)).violations).toEqual([]);
    fireEvent.click(header());
    expect((await axe(container, AXE_RULES)).violations).toEqual([]);
  });

  it("drops the chevron transition under reduced motion", async () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((q: string) => ({ matches: q.includes("reduce"), media: q, addEventListener() {}, removeEventListener() {} }))
    );
    window.matchMedia = globalThis.matchMedia;
    const { container } = await renderWith([entry({ hasRecording: false })]);
    const chevron = container.querySelector("[data-chevron]") as SVGElement;
    expect(chevron.style.transition).toBe("none");
  });
});

describe("waveform", () => {
  it("splits into the requested bucket count and normalises to the loudest", () => {
    const s = new Int16Array(1000);
    for (let i = 500; i < 1000; i++) s[i] = i % 2 ? 16000 : -16000;
    const peaks = peaksFromSamples(s, 10);
    expect(peaks).toHaveLength(10);
    expect(Math.max(...peaks)).toBeCloseTo(1);
    expect(peaks[0]).toBe(0);
  });

  it("fits the bar count to the row and keeps each bucket's loudest bar", () => {
    // About 370 px of row at the 900 px window minimum: under 96 bars.
    expect(barsForWidth(370)).toBe(93);
    expect(barsForWidth(370) * 4 - 2).toBeLessThanOrEqual(370);
    expect(barsForWidth(2000)).toBe(96);
    expect(barsForWidth(0)).toBe(96);
    expect(resamplePeaks([0.1, 0.9, 0.2, 0.3], 2)).toEqual([0.9, 0.3]);
    expect(resamplePeaks([0.1, 0.2], 4)).toEqual([0.1, 0.2]);
  });

  it("finds the data chunk after an extra chunk and reports the duration", () => {
    const wav = makeWav(new Float32Array(8000).fill(0.25), 16000, true);
    const parsed = parseWav(wav);
    expect(parsed?.samples.length).toBe(8000);
    const wave = waveformFromWav(wav);
    expect(wave?.peaks).toHaveLength(96);
    expect(wave?.durationSec).toBeCloseTo(0.5);
    expect(parseWav(new ArrayBuffer(4))).toBeNull();
  });
});
