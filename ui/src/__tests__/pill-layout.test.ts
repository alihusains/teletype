// claim: ui-behavior
// Pure layout math behind the existing-pill layout fixes
// (research/pills/PILL_SPEC.md section 11).
import { describe, expect, it } from "vitest";
import { busyPillSize, NOTE_TWO_LINE_HEIGHT, PROCESSING_MAX_WIDTH } from "../lib/pillLayout";
import { SIZES } from "../lib/rainbow";

// A deterministic stand-in for canvas measureText: 7px per character.
const measure = (text: string) => text.length * 7;

describe("H4: busy pill width follows its message", () => {
  it("keeps the 220px floor for a short status", () => {
    expect(
      busyPillSize({ message: "Transcribing…", skipMessage: null, micFailure: false, hasAction: false }, measure),
    ).toEqual({ width: SIZES.processing.width, height: SIZES.processing.height, lines: 1 });
  });

  it("grows past 220px for a longer note instead of truncating it", () => {
    const message = "Paste failed. Text kept for later"; // 33 chars, 231px
    const size = busyPillSize({ message, skipMessage: null, micFailure: false, hasAction: false }, measure);
    expect(size.width).toBe(32 + 231);
    expect(size.lines).toBe(1);
  });

  it("gives a settings-fixable error at least the 340px mic width plus the action", () => {
    const size = busyPillSize(
      { message: "Device not found", skipMessage: null, micFailure: false, hasAction: true },
      measure,
    );
    expect(size.width).toBeGreaterThanOrEqual(SIZES.processingMic.width);
    // 32 padding + 112 text + 10 gap + 26 chrome + 91 "Open Settings".
    expect(size.width).toBe(Math.max(SIZES.processingMic.width, 32 + 112 + 10 + 26 + 91));
  });

  it("wraps the model-load error to two clamped lines at the 480px cap", () => {
    const size = busyPillSize(
      {
        message: "Couldn't load the speech model (model file is missing, re-download it in Settings)",
        skipMessage: null,
        micFailure: false,
        hasAction: true,
      },
      measure,
    );
    expect(size).toEqual({ width: PROCESSING_MAX_WIDTH, height: NOTE_TWO_LINE_HEIGHT, lines: 2 });
  });

  it("never exceeds the cap, whatever the message length", () => {
    const size = busyPillSize(
      { message: "x".repeat(400), skipMessage: null, micFailure: false, hasAction: false },
      measure,
    );
    expect(size.width).toBeLessThanOrEqual(PROCESSING_MAX_WIDTH);
  });
});
