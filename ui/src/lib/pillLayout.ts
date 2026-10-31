// Pure layout math for the recorder pill (ui/src/pill.tsx). The legacy busy
// chrome reports a computed size to Rust before it renders (the OS window is
// sized to the pill box), so its width has to come from the text, not from a
// post-render measurement. Kept free of React so it can be unit tested.
import { SIZES } from "./rainbow";

/** Widest the busy pill grows before its text wraps to a second line. */
export const PROCESSING_MAX_WIDTH = 480;

/** Busy pill height when the message needs two lines. */
export const NOTE_TWO_LINE_HEIGHT = 64;

const FONT_STACK = '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
/** The busy message font (LegacyProcessingBody). */
export const NOTE_FONT = `500 13px ${FONT_STACK}`;
/** The "Open Settings" action font (SettingsAction / MicFailureAction). */
export const ACTION_FONT = `600 12px ${FONT_STACK}`;

const NOTE_PADDING_X = 32; // 16px each side
const ACTION_GAP = 10; // flex gap between the message and the action
const ACTION_CHROME = 26; // 12px padding each side plus a 1px border each side

/** Returns the rendered width of `text` in `font`, in CSS px. */
export type TextMeasure = (text: string, font: string) => number;

let measureCtx: OffscreenCanvasRenderingContext2D | null | undefined;

/** Canvas `measureText`, with a per-character estimate where no canvas exists
 * (jsdom). The estimate errs wide so a test never under-sizes a pill. */
export function measureTextPx(text: string, font: string): number {
  if (measureCtx === undefined) {
    try {
      measureCtx =
        typeof OffscreenCanvas !== "undefined" ? new OffscreenCanvas(1, 1).getContext("2d") : null;
    } catch {
      measureCtx = null;
    }
  }
  if (!measureCtx) {
    const px = Number.parseFloat(/(\d+(?:\.\d+)?)px/.exec(font)?.[1] ?? "13");
    return text.length * px * 0.6;
  }
  measureCtx.font = font;
  return measureCtx.measureText(text).width;
}

export interface BusyPillInput {
  message: string;
  skipMessage: string | null;
  micFailure: boolean;
  /** True when an "Open Settings" action renders beside the message. */
  hasAction: boolean;
}

export interface BusyPillSize {
  width: number;
  height: number;
  /** Lines the message is laid out on (2 means clamped, full text in title). */
  lines: 1 | 2;
}

/** The legacy busy pill's size for a message: content-sized between the
 * 220px floor and the 480px cap, with room for the action when one renders.
 * A message that still overflows at the cap wraps to two clamped lines. */
export function busyPillSize(input: BusyPillInput, measure: TextMeasure = measureTextPx): BusyPillSize {
  if (input.skipMessage) {
    return {
      width: input.micFailure ? SIZES.processingMic.width : SIZES.processingSkip.width,
      height: SIZES.processing.height,
      lines: 1,
    };
  }
  const action = input.hasAction
    ? ACTION_GAP + ACTION_CHROME + Math.ceil(measure("Open Settings", ACTION_FONT))
    : 0;
  const natural = NOTE_PADDING_X + Math.ceil(measure(input.message, NOTE_FONT)) + action;
  // An action always gets at least the mic-failure width, the size the
  // release audit found legible for a short error beside "Open Settings".
  const floor = input.hasAction ? SIZES.processingMic.width : SIZES.processing.width;
  if (natural <= PROCESSING_MAX_WIDTH) {
    return { width: Math.max(floor, natural), height: SIZES.processing.height, lines: 1 };
  }
  return { width: PROCESSING_MAX_WIDTH, height: NOTE_TWO_LINE_HEIGHT, lines: 2 };
}

/** Height of one row in the inline input-device list (a 24px hit target). */
export const DEVICE_ROW_HEIGHT = 24;
/** The device list scrolls past this height. */
export const DEVICE_LIST_MAX_HEIGHT = 140;
/** Gap between the default capsule and its device list. */
export const DEVICE_LIST_GAP = 4;
const DEVICE_LIST_PADDING_Y = 8; // 4px top and bottom

/** Exact height of the inline device list for `count` devices, so the legacy
 * pill (which reports a computed size) can grow the window to hold it. */
export function deviceListHeight(count: number): number {
  return Math.min(DEVICE_LIST_MAX_HEIGHT, DEVICE_LIST_PADDING_Y + Math.max(0, count) * DEVICE_ROW_HEIGHT);
}

export interface PillAnchor {
  v: "top" | "center" | "bottom";
  h: "left" | "center" | "right";
}

/** The edges overlay.rs keeps fixed for a pill position (wire id such as
 * "bottomCenter"). Unknown ids resolve to bottom centre, like
 * `PillPositionSetting::resolve` in Rust. */
export function pillAnchor(position: string | undefined): PillAnchor {
  return PILL_ANCHORS[position ?? ""] ?? { v: "bottom", h: "center" };
}

const PILL_ANCHORS: Record<string, PillAnchor> = {
  topLeft: { v: "top", h: "left" },
  topCenter: { v: "top", h: "center" },
  topRight: { v: "top", h: "right" },
  centerLeft: { v: "center", h: "left" },
  center: { v: "center", h: "center" },
  centerRight: { v: "center", h: "right" },
  bottomLeft: { v: "bottom", h: "left" },
  bottomCenter: { v: "bottom", h: "center" },
  bottomRight: { v: "bottom", h: "right" },
};
