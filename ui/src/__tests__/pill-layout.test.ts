// claim: ui-behavior
// Pure layout math behind the existing-pill layout fixes
// (research/pills/PILL_SPEC.md section 11).
import { describe, expect, it } from "vitest";
import {
  busyPillSize,
  interimTail,
  pillAnchor,
  NOTE_TWO_LINE_HEIGHT,
  planLegacyReport,
  PROCESSING_MAX_WIDTH,
} from "../lib/pillLayout";
import { SIZES } from "../lib/rainbow";
import { DOT_COLS, sequenceActive, sequenceAnimates, staticFrame } from "../lib/dotGrid";

const litCount = (frame: boolean[][]) => frame.flat().filter(Boolean).length;
const litKey = (frame: boolean[][]) =>
  frame.flatMap((row, y) => row.flatMap((on, x) => (on ? [`${x},${y}`] : []))).join(" ");

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

describe("H5: dot grid sequences step and have legible still frames", () => {
  it("advances the row scan and ring sweep from step to step", () => {
    for (const state of ["thinking", "connecting"] as const) {
      expect(litKey(sequenceActive(state, 0))).not.toBe(litKey(sequenceActive(state, 1)));
      expect(litCount(sequenceActive(state, 7))).toBe(1);
    }
  });

  it("only the sequence states run a step timer", () => {
    expect(sequenceAnimates("thinking")).toBe(true);
    expect(sequenceAnimates("connecting")).toBe(true);
    expect(sequenceAnimates("listening")).toBe(true);
    expect(sequenceAnimates("speaking")).toBe(false);
    expect(sequenceAnimates("idle")).toBe(false);
  });

  it("reduced motion lights the whole middle row for thinking", () => {
    const frame = staticFrame("thinking");
    expect(litCount(frame)).toBe(DOT_COLS);
    expect(frame[3].every(Boolean)).toBe(true);
  });

  it("reduced motion lights the full ring for connecting", () => {
    const frame = staticFrame("connecting");
    // The ring is the grid's outer border on a 25x7 grid: 2*25 + 2*5 dots.
    expect(litCount(frame)).toBe(2 * 25 + 2 * 5);
    expect(frame[0].every(Boolean)).toBe(true);
    expect(frame[3][12]).toBe(false);
  });
});

describe("M3: the legacy pill reports a shrink only after it animates", () => {
  const big = { w: 540, h: 52 };
  const small = { w: 250, h: 52 };

  it("reports a grow at once", () => {
    expect(planLegacyReport({ reported: small, target: big, capsuleShrinking: false, inFlight: false })).toEqual({
      now: big,
      deferred: false,
    });
  });

  it("keeps the previous size while the capsule shrinks", () => {
    expect(planLegacyReport({ reported: big, target: small, capsuleShrinking: true, inFlight: false })).toEqual({
      now: big,
      deferred: true,
    });
  });

  it("holds the union when one side grows and the other shrinks", () => {
    expect(
      planLegacyReport({
        reported: { w: 480, h: 64 },
        target: { w: 540, h: 52 },
        capsuleShrinking: true,
        inFlight: false,
      }),
    ).toEqual({ now: { w: 540, h: 64 }, deferred: true });
  });

  it("stays deferred across an unrelated re-render while the shrink animates", () => {
    expect(planLegacyReport({ reported: big, target: small, capsuleShrinking: false, inFlight: true }).deferred).toBe(
      true,
    );
  });

  it("a window-only shrink (device list closing) is reported at once", () => {
    expect(
      planLegacyReport({ reported: { w: 540, h: 112 }, target: big, capsuleShrinking: false, inFlight: false }),
    ).toEqual({ now: big, deferred: false });
  });
});

describe("M4: the Level Rail shows the newest words", () => {
  it("keeps a short transcript as is", () => {
    expect(interimTail("Hello there.")).toBe("Hello there.");
  });

  it("keeps the tail of a long transcript, cut on a word, punctuation intact", () => {
    const tail = interimTail("So the plan for the quarterly review is to walk through the funnel first.");
    expect(tail.startsWith("…")).toBe(true);
    expect(tail.endsWith("funnel first.")).toBe(true);
    expect(tail.length).toBeLessThanOrEqual(41);
    expect(tail[1]).not.toBe(" ");
  });

  it("falls back to a hard cut when the tail has no space", () => {
    expect(interimTail("x".repeat(60), 40)).toBe("…" + "x".repeat(40));
  });
});

describe("M5: the pill content keeps the edge Rust anchors", () => {
  it("maps every slot to its fixed edges", () => {
    expect(pillAnchor("topLeft")).toEqual({ v: "top", h: "left" });
    expect(pillAnchor("center")).toEqual({ v: "center", h: "center" });
    expect(pillAnchor("centerRight")).toEqual({ v: "center", h: "right" });
    expect(pillAnchor("bottomCenter")).toEqual({ v: "bottom", h: "center" });
  });

  it("resolves an unknown or missing slot to bottom centre, like Rust", () => {
    expect(pillAnchor(undefined)).toEqual({ v: "bottom", h: "center" });
    expect(pillAnchor("somewhereNew")).toEqual({ v: "bottom", h: "center" });
  });
});

describe("M7: a skip notice gets its own line at full width", () => {
  for (const micFailure of [false, true]) {
    it(`stacks under the status at 480x64 (mic failure: ${micFailure})`, () => {
      expect(
        busyPillSize(
          { message: "Inserting…", skipMessage: "Polish skipped: no model loaded. Using raw text", micFailure, hasAction: micFailure },
          measure,
        ),
      ).toEqual({ width: PROCESSING_MAX_WIDTH, height: 64, lines: 1 });
    });
  }
});
