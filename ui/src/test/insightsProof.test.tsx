// claim: ui-behavior
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invokeCalls, mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

import InsightsScreen, { type Insights } from "../screens/InsightsScreen";

function fixture(overrides: Partial<Insights> = {}): Insights {
  const day = 86_400_000;
  return {
    topPhrases: [{ label: "thanks for the update", count: 6, share: 1 }],
    topApps: [{ label: "Slack", count: 8, share: 1 }],
    appCategories: [
      { label: "Chat", count: 8, share: 1 },
      { label: "Email", count: 4, share: 0.5 },
      { label: "Code", count: 2, share: 0.25 },
    ],
    avgWordsPerDictation: 100,
    busiestHour: 9,
    topDays: [
      { label: "Monday", count: 400, share: 1.0 },
      { label: "Wednesday", count: 300, share: 0.75 },
      { label: "Friday", count: 200, share: 0.5 },
    ],
    bestDay: "Monday",
    hourBands: [
      { label: "Morning", count: 600, share: 1.0 },
      { label: "Afternoon", count: 400, share: 0.67 },
      { label: "Evening", count: 150, share: 0.25 },
      { label: "Late night", count: 50, share: 0.083 },
    ],
    topWords: [
      { label: "project", count: 88, share: 1.0 },
      { label: "client", count: 64, share: 0.73 },
      { label: "review", count: 42, share: 0.48 },
    ],
    habitWords: {
      total: 5,
      per1kWords: 4.2,
      items: [{ label: "like", count: 3, share: 1.0 }, { label: "basically", count: 2, share: 0.67 }],
      trend: 0,
    },
    vocabularySize: 410,
    dictionaryWords: 12,
    learnedWords: 4,
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
    records: [{ label: "Longest take", value: "312 words" }],
    milestones: [
      {
        category: "Words",
        current: 1200,
        items: [{ label: "1K", threshold: 1000, reached: true }],
      },
    ],
    heatmap: [{ date: 1_792_000_000_000, words: 120, level: 2 }],
    daily: [{ label: "16 Sep", date: 1_792_000_000_000, words: 120, dictations: 2 }],
    wordsToday: 40,
    wordsLast7Days: 800,
    avgWordsPerDay: 171,
    firstActivityAt: 1_790_000_000_000,
    fillerRemoved: 23,
    autotextExpansions: 7,
    topAutotext: [
      { label: "/email", count: 9, share: 1 },
      { label: "my signature", count: 4, share: 0.44 },
    ],
    topFillers: [
      { label: "um", count: 12, share: 1 },
      { label: "uh", count: 6, share: 0.5 },
    ],
    personalizationCorrections: 5,
    dictionaryLearned: 3,
    polishStatus: null,
    learning: {
      topFixes: [{ from: "corey", to: "Cory", count: 4 }],
      termsLearnedWeek: [{ term: "Kubernetes" }],
      editRate7d: 2.4,
      daily: [
        { day: 1_792_000_000_000 - day, words: 200, editedWords: 8 },
        { day: 1_792_000_000_000, words: 150, editedWords: 3 },
      ],
      accuracy: [{ ts: 1_792_000_000_000, werBefore: 0.12, werAfter: 0.08 }],
    },
    ...overrides,
  };
}

async function mount(data = fixture(), onNavigate?: (screen: string) => void) {
  mockInvokeResponses({ get_insights: data });
  mockListenCapture();
  render(<InsightsScreen onNavigate={onNavigate} />);
  await waitFor(() => expect(invokeCalls().some((c) => c.cmd === "get_insights")).toBe(true));
}

function insightsCalls() {
  return invokeCalls().filter((c) => c.cmd === "get_insights");
}

describe("Insights proof screen", () => {
  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("opens on this week and asks the backend for that window", async () => {
    await mount();
    expect(insightsCalls()[0]).toEqual({
      cmd: "get_insights",
      args: { range: "week", customFrom: null, customTo: null },
    });
    expect(screen.getByRole("button", { name: "7 days" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("leads with the speed multiplier and proves it against typing", async () => {
    await mount();
    expect(await screen.findByText("2.5×")).toBeTruthy();
    expect(screen.getByText("faster than typing")).toBeTruthy();
    // Time saved is the supporting figure, not the headline. It appears in
    // both the hero sentence and the glance tile.
    expect(screen.getAllByText("18 min").length).toBeGreaterThan(0);
    expect(screen.getByText(/words dictated/)).toBeTruthy();
    expect(screen.getByText("You, speaking")).toBeTruthy();
    expect(screen.getByText("Typing average")).toBeTruthy();
    expect(screen.getByText("40 wpm")).toBeTruthy();
    // Measured speaking and typing time are both surfaced.
    expect(screen.getByText(/Speaking time/)).toBeTruthy();
    expect(screen.getByText(/Typing time/)).toBeTruthy();
  });

  it("re-asks the backend when the range changes", async () => {
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "30 days" }));
    await waitFor(() =>
      expect(insightsCalls().some((c) => (c.args as { range?: string }).range === "month")).toBe(true)
    );
  });

  it("shows every insight section without a disclosure toggle", async () => {
    await mount();
    for (const title of [
      "Words per day",
      "Where you dictate",
      "Work mix",
      "Automations",
      "Your rhythm",
      "Time of day",
      "Phrases you repeat",
      "Language habit",
      "Records",
      "Milestones",
    ]) {
      expect(await screen.findByText(title)).toBeTruthy();
    }
    expect(screen.queryByRole("button", { name: /Show|Hide/ })).toBeNull();
  });

  it("compares the window with the one before it and shows active days", async () => {
    await mount();
    expect(await screen.findAllByText("Up 23% on the previous window")).toBeTruthy();
    expect(screen.getByText("5/7")).toBeTruthy();
    expect(screen.getByText(/975 words before/)).toBeTruthy();
  });

  it("renders the learning trends the backend computes", async () => {
    await mount();
    expect(await screen.findByText("Personal accuracy")).toBeTruthy();
    expect(screen.getByText("12.0%")).toBeTruthy();
    expect(screen.getByText("8.0%")).toBeTruthy();
    expect(screen.getByText("Edits per 100 words · last 30 days")).toBeTruthy();
    expect(screen.getByText("9×")).toBeTruthy(); // top AutoText macro
    expect(screen.getByText("um")).toBeTruthy(); // filler breakdown
    expect(screen.getByText("Chat")).toBeTruthy(); // work mix category
  });

  it("keeps records and milestones visible in the default window", async () => {
    await mount();
    expect(await screen.findByText("Records")).toBeTruthy();
    expect(screen.getByText("Longest take")).toBeTruthy();
    expect(screen.getByText("Milestones")).toBeTruthy();
    expect(screen.getByText(/1K/)).toBeTruthy();
  });

  it("offers all time from an empty window and switches to it", async () => {
    await mount(
      fixture({
        totalWords: 0,
        totalDictations: 0,
        impact: {
          wordsPerMinute: null,
          timesFaster: null,
          timeSavedMinutes: 0,
          timeSavedLabel: "0 min",
          minutesSpoken: null,
          minutesTyped: 0,
          essays: 0,
          ratedTakes: 0,
          skippedTakes: 0,
        },
      })
    );
    expect(await screen.findByText(/No dictations in the last 7 days\./)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "See all time" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "All time" }).getAttribute("aria-pressed")).toBe("true")
    );
  });

  it("routes each personalisation CTA to its screen", async () => {
    const onNavigate = vi.fn();
    await mount(fixture(), onNavigate);

    fireEvent.click(await screen.findByRole("button", { name: "Dictionary" }));
    expect(onNavigate).toHaveBeenCalledWith("dictionary");

    fireEvent.click(screen.getByRole("button", { name: "AutoText" }));
    expect(onNavigate).toHaveBeenCalledWith("autotext");

    fireEvent.click(screen.getByRole("button", { name: "Tune AI Polish" }));
    expect(onNavigate).toHaveBeenCalledWith("transforms");
  });

  it("shows the learning and personalisation proof the backend computes", async () => {
    await mount();
    expect(await screen.findAllByText("8")).toBeTruthy(); // 5 corrections + 3 learned
    expect(screen.getByText("Kubernetes")).toBeTruthy();
    expect(screen.getByText("2.4")).toBeTruthy();
    expect(screen.getByText("Cory")).toBeTruthy();
  });

  it("shows the rhythm card with best day, top days, and busiest hour band", async () => {
    await mount();
    expect(await screen.findByText("Your rhythm")).toBeTruthy();
    expect(screen.getByText("Your best day is Monday")).toBeTruthy();
    expect(screen.getByText("Most of your words land in the morning.")).toBeTruthy();
    const timeCard = screen.getByText("Time of day").closest(".tt-card") as HTMLElement;
    expect(within(timeCard).getByText(/Busiest single hour:/)).toBeTruthy();
    expect(within(timeCard).getByText("09:00 UTC")).toBeTruthy();
  });

  it("shows the language habit card without expanding anything", async () => {
    await mount();
    expect(await screen.findByText("Language habit")).toBeTruthy();
    expect(screen.getByText("Your top words")).toBeTruthy();
    expect(screen.getByText("project")).toBeTruthy();
    expect(screen.getByText("4.2")).toBeTruthy(); // per1kWords
    expect(screen.getByText(/steady/)).toBeTruthy(); // trend 0
  });

  it("shows the habit words zero state when no habit words detected", async () => {
    await mount(fixture({
      habitWords: { total: 0, per1kWords: 0, items: [], trend: 0 },
    }));
    expect(await screen.findByText(/No habit words detected/)).toBeTruthy();
  });

  it("shows the rhythm card empty state when no words", async () => {
    await mount(fixture({
      totalWords: 0,
      totalDictations: 0,
      topDays: [],
      bestDay: null,
      hourBands: [],
      topWords: [],
      habitWords: { total: 0, per1kWords: 0, items: [], trend: 0 },
      impact: {
        wordsPerMinute: null,
        timesFaster: null,
        timeSavedMinutes: 0,
        timeSavedLabel: "0 min",
        minutesSpoken: null,
        minutesTyped: 0,
        essays: 0,
        ratedTakes: 0,
        skippedTakes: 0,
      },
    }));
    expect(await screen.findByText("Your rhythm")).toBeTruthy();
    const rhythmCard = screen.getByText("Your rhythm").closest(".tt-card") as HTMLElement;
    expect(within(rhythmCard).getByText(/Dictate a few takes/)).toBeTruthy();
  });

  it("shows an empty state for automations when nothing has fired", async () => {
    await mount(fixture({ topAutotext: [], topFillers: [] }));
    expect(await screen.findByText(/No AutoText or filler activity yet/)).toBeTruthy();
  });

  it("renders an all-empty payload without crashing", async () => {
    await mount({
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
      totalWords: 0,
      totalDictations: 0,
      streakDays: 0,
      longestStreakDays: 0,
      activeDaysLast7: 0,
      priorWindowWords: null,
      wordsChangePct: null,
      impact: {
        wordsPerMinute: null,
        timesFaster: null,
        timeSavedMinutes: 0,
        timeSavedLabel: "0 min",
        minutesSpoken: null,
        minutesTyped: 0,
        essays: 0,
        ratedTakes: 0,
        skippedTakes: 0,
      },
      records: [],
      milestones: [],
      heatmap: [],
      daily: [],
      wordsToday: 0,
      wordsLast7Days: 0,
      avgWordsPerDay: 0,
      firstActivityAt: null,
      fillerRemoved: 0,
      autotextExpansions: 0,
      topAutotext: [],
      topFillers: [],
      personalizationCorrections: 0,
      dictionaryLearned: 0,
      polishStatus: null,
      learning: null,
    });
    expect(await screen.findByText("Insights")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getAllByText(/No activity yet|No records yet|No milestones yet|No app attribution yet/).length).toBeGreaterThan(0);
  });
});
