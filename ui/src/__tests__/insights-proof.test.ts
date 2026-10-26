// claim: ui-behavior
import { describe, expect, it } from "vitest";
import {
  formatMinutes,
  rangeLearnedLabel,
  rangeWindowLabel,
  speedScale,
  spokenMinutes,
  writtenTally,
} from "../screens/InsightsScreen";

describe("insights proof helpers", () => {
  it("formats minutes without ever showing a second unit at zero", () => {
    expect(formatMinutes(0)).toBe("0 min");
    expect(formatMinutes(-5)).toBe("0 min");
    expect(formatMinutes(59)).toBe("59 min");
    expect(formatMinutes(60)).toBe("1 hr");
    expect(formatMinutes(61)).toBe("1 hr 1 min");
    expect(formatMinutes(125.4)).toBe("2 hr 5 min");
  });

  it("marks speaking time estimated only when it was not measured", () => {
    expect(spokenMinutes({ minutesSpoken: 4.2, minutesTyped: 20, timeSavedMinutes: 15.8 })).toEqual({
      minutes: 4.2,
      estimated: false,
    });
    // The backend's 150 wpm fallback: typing time minus the saved minutes.
    expect(spokenMinutes({ minutesSpoken: null, minutesTyped: 40, timeSavedMinutes: 24 })).toEqual({
      minutes: 16,
      estimated: true,
    });
    // No words at all: nothing to call an estimate.
    expect(spokenMinutes({ minutesSpoken: null, minutesTyped: 0, timeSavedMinutes: 0 })).toEqual({
      minutes: 0,
      estimated: false,
    });
  });

  it("scales the speed bars so the slower bar never fills the track", () => {
    // 40 wpm baseline with 8% headroom.
    expect(speedScale(null)).toBeCloseTo(43.2);
    expect(speedScale(30)).toBeCloseTo(43.2);
    expect(speedScale(150)).toBeCloseTo(162);
  });

  it("phrases each window for the sentence it appears in", () => {
    expect(rangeWindowLabel("week")).toBe("the last 7 days");
    expect(rangeWindowLabel("month")).toBe("the last 30 days");
    expect(rangeWindowLabel("year")).toBe("the last 12 months");
    expect(rangeWindowLabel("lifetime")).toBe("all time");
    expect(rangeWindowLabel("custom")).toBe("the selected range");

    expect(rangeLearnedLabel("week")).toBe("in the last 7 days");
    expect(rangeLearnedLabel("lifetime")).toBe("all time");
    expect(rangeLearnedLabel("custom")).toBe("in this range");
  });

  it("escalates the written tally with volume and stays silent below one", () => {
    expect(writtenTally(0)).toBeNull();
    expect(writtenTally(499)).toBeNull();
    expect(writtenTally(500)).toBe("You've written 1 college essay!");
    expect(writtenTally(1_499)).toBe("You've written 2 college essays!");
    expect(writtenTally(1_600)).toBe("You've written 1 magazine article!");
    expect(writtenTally(5_000)).toBe("You've written 1 blog post!");
    expect(writtenTally(90_000)).toBe("You've written 1 book!");
    expect(writtenTally(270_000)).toBe("You've written 3 books!");
  });
});
