import { describe, expect, it } from "vitest";
import { search } from "./settingsSearch";

const LABELS = [
  "Record hotkey",
  "Recording mode",
  "Restore the previous clipboard",
  "Language",
  "Noise reduction",
  "Reduce Motion",
  "Spoken punctuation",
  "café",
];

describe("search", () => {
  it("returns nothing for an empty or whitespace query", () => {
    expect(search(LABELS, "")).toEqual([]);
    expect(search(LABELS, "   ")).toEqual([]);
  });

  it("matches case-insensitively", () => {
    const m = search(LABELS, "record hotkey");
    expect(m.map((x) => x.label)).toContain("Record hotkey");
    expect(search(LABELS, "noisE").map((x) => x.label)).toContain("Noise reduction");
  });

  it("matches a subsequence, not just a prefix", () => {
    const m = search(LABELS, "nr");
    expect(m.map((x) => x.label)).toContain("Noise reduction");
  });

  it("does not match when characters are out of order", () => {
    expect(search(LABELS, "eudcer")).toEqual([]);
    expect(search(LABELS, "xyz")).toEqual([]);
  });

  it("is diacritic-tolerant on both sides", () => {
    expect(search(LABELS, "cafe").map((x) => x.label)).toContain("café");
    expect(search(["Accents"], "ACCENTS").length).toBe(1);
    expect(search(LABELS, "caf\u0301").map((x) => x.label)).toContain("café");
  });

  it("ranks contiguous word-start runs above scattered letters", () => {
    const m = search(LABELS, "rec");
    // "Record hotkey" and "Recording mode" both open with a contiguous
    // word-start run; the scattered match must not beat them.
    expect(m[0].label).toBe("Record hotkey");
    expect(m[1].label).toBe("Recording mode");
    expect(m[0].score).toBeGreaterThan(m[m.length - 1].score);
  });

  it("ranks an exact word-start run above a mid-word run", () => {
    const m = search(LABELS, "reduce");
    expect(m[0].label).toBe("Reduce Motion");
  });

  it("does not report a label as a false positive", () => {
    const m = search(["Smart insertion"], "sssmmarrtt");
    expect(m).toEqual([]);
  });
});
