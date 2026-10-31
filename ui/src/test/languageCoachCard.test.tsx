// claim: ui-behavior
// T20: strong-language coach as a separate Insights card.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, waitFor, within } from "@testing-library/react";
import { invokeCalls, mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

import InsightsScreen, { type Insights } from "../screens/InsightsScreen";

function fixture(overrides: Partial<Insights> = {}): Insights {
  return {
    topPhrases: [],
    topApps: [],
    appCategories: [],
    avgWordsPerDictation: 0,
    busiestHour: null,
    topDays: [],
    bestDay: null,
    hourBands: [],
    topWords: [],
    habitWords: { total: 0, per1kWords: 0, items: [], trend: 0 },
    vocabularySize: 0,
    dictionaryWords: 0,
    learnedWords: 0,
    totalWords: 1200,
    totalDictations: 12,
    streakDays: 3,
    longestStreakDays: 9,
    activeDaysLast7: 5,
    priorWindowWords: 975,
    wordsChangePct: 23.4,
    impact: {
      wordsPerMinute: 100,
      timesFaster: 2.5,
      timeSavedMinutes: 18,
      timeSavedLabel: "18 min",
      minutesSpoken: 12,
      minutesTyped: 30,
      essays: 2,
      ratedTakes: 10,
      skippedTakes: 2,
    },
    records: [],
    milestones: [],
    heatmap: [],
    daily: [],
    wordsToday: 40,
    wordsLast7Days: 800,
    avgWordsPerDay: 171,
    firstActivityAt: 1_790_000_000_000,
    fillerRemoved: 0,
    autotextExpansions: 0,
    topAutotext: [],
    topFillers: [],
    personalizationCorrections: 0,
    dictionaryLearned: 0,
    polishStatus: null,
    learning: null,
    ...overrides,
  };
}

function languageFixture(overrides: Partial<Insights["language"]> = {}): Insights["language"] {
  return {
    source: "spoken",
    mild: 2,
    strong: 3,
    slur: 0,
    work: { mild: 1, strong: 2, slur: 0 },
    personal: { mild: 1, strong: 1, slur: 0 },
    other: { mild: 0, strong: 0, slur: 0 },
    topWords: [
      { label: "f***", count: 2 },
      { label: "d***", count: 2 },
    ],
    trend: "up",
    cleanDaysStreak: 12,
    ...overrides,
  };
}

async function mount(
  data: Insights,
  settings: { showLanguageCoach: boolean } = { showLanguageCoach: true }
) {
  mockInvokeResponses({
    get_insights: data,
    get_settings: settings,
  });
  mockListenCapture();
  render(<InsightsScreen />);
  await waitFor(() =>
    expect(invokeCalls().some((c) => c.cmd === "get_insights")).toBe(true)
  );
}

/** Find the Language coach card: the .tt-card whose h3 title is "Language". */
function findLanguageCard(): HTMLElement {
  const cards = Array.from(document.querySelectorAll(".tt-card"));
  const card = cards.find((c) => {
    const h3 = c.querySelector("h3");
    return h3?.textContent?.trim() === "Language";
  });
  if (!card) throw new Error("Language coach card not found");
  return card as HTMLElement;
}

describe("Language coach card (T20)", () => {
  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("renders the headline, masked chips, and the work/personal split", async () => {
    // work.strong = 0 so the headline is the clean-days-streak variant.
    await mount(
      fixture({
        language: languageFixture({
          strong: 1,
          work: { mild: 1, strong: 0, slur: 0 },
          personal: { mild: 1, strong: 1, slur: 0 },
          topWords: [
            { label: "f***", count: 1 },
            { label: "d***", count: 1 },
          ],
        }),
      })
    );
    const cardEl = findLanguageCard();
    // Headline about the user, non-judgmental: no strong language at work
    // for the clean-days streak.
    expect(within(cardEl).getByText(/No strong language at work for 12 days/)).toBeTruthy();
    // Masked chips, never the raw words.
    expect(within(cardEl).getByText("f***")).toBeTruthy();
    expect(within(cardEl).getByText("d***")).toBeTruthy();
    const cardText = cardEl.textContent ?? "";
    expect(cardText).not.toContain("fuck");
    expect(cardText).not.toContain("damn");
    // Work vs personal split: the number is in a <b> tag, so match the text after it.
    expect(within(cardEl).getByText("strong in work")).toBeTruthy();
    expect(within(cardEl).getByText("strong in personal")).toBeTruthy();
  });

  it("shows the trend direction", async () => {
    await mount(fixture({ language: languageFixture({ trend: "down" }) }));
    const cardEl = findLanguageCard();
    expect(within(cardEl).getByText(/less than the period before/)).toBeTruthy();
  });

  it("is hidden entirely when showLanguageCoach is off", async () => {
    await mount(fixture({ language: languageFixture() }), { showLanguageCoach: false });
    const cards = Array.from(document.querySelectorAll(".tt-card"));
    const languageCards = cards.filter((c) => {
      const h3 = c.querySelector("h3");
      return h3?.textContent?.trim() === "Language";
    });
    expect(languageCards.length).toBe(0);
  });

  it("shows an empty state when there is no data", async () => {
    await mount(
      fixture({
        totalWords: 0,
        totalDictations: 0,
        language: languageFixture({
          mild: 0,
          strong: 0,
          slur: 0,
          work: { mild: 0, strong: 0, slur: 0 },
          personal: { mild: 0, strong: 0, slur: 0 },
          other: { mild: 0, strong: 0, slur: 0 },
          topWords: [],
          trend: "flat",
          cleanDaysStreak: 0,
        }),
      })
    );
    const cardEl = findLanguageCard();
    expect(within(cardEl).getByText(/Nothing to show yet/)).toBeTruthy();
  });

  it("says when the counts come from final text instead of speech", async () => {
    await mount(fixture({ language: languageFixture({ source: "final" }) }));
    const cardEl = findLanguageCard();
    expect(within(cardEl).getByText(/from your saved text/)).toBeTruthy();
  });
});

describe("maskWord", () => {
  it("keeps the first letter and masks the rest", async () => {
    const { maskWord } = await import("../screens/InsightsScreen");
    expect(maskWord("damn")).toBe("d***");
    expect(maskWord("a")).toBe("a");
  });
});
