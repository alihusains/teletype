// Frame math for the Dot Matrix pill style (DotGridMatrix in ui/src/pill.tsx).
//
// Ported from LiveKit's Agents UI (github.com/livekit/components-js,
// packages/shadcn), Apache-2.0 licensed. The original drives a dot grid from
// live agent state and multiband track volume; we keep the same sequence
// geometry (ring sweep, center pulse, row scan, volume rows) but drive it from
// Teletype's pill phases and the scalar `pill-level` stream instead. Files
// modified from the original: the reduced-motion static frames are ours.

export type DotGridState = "idle" | "connecting" | "listening" | "speaking" | "thinking";

interface Coordinate {
  x: number;
  y: number;
}

// Dot matrix grid: 25×7.
export const DOT_COLS = 25;
export const DOT_ROWS = 7;

/** How often a sequence state (ring sweep, pulse, row scan) advances. */
export const DOT_STEP_MS = 80;

// LiveKit's AgentAudioVisualizerGrid (Apache-2.0) sequences, ported verbatim
// except for formatting: ring sweep for connecting, a center pulse with
// blank beats for listening, a left-right row scan for thinking.
function generateConnectingSequence(rows: number, columns: number, radius: number): Coordinate[] {
  const seq: Coordinate[] = [];
  const centerY = Math.floor(rows / 2);
  const topLeft = { x: Math.max(0, centerY - radius), y: Math.max(0, centerY - radius) };
  const bottomRight = {
    x: columns - 1 - topLeft.x,
    y: Math.min(rows - 1, centerY + radius),
  };
  for (let x = topLeft.x; x <= bottomRight.x; x++) seq.push({ x, y: topLeft.y });
  for (let y = topLeft.y + 1; y <= bottomRight.y; y++) seq.push({ x: bottomRight.x, y });
  for (let x = bottomRight.x - 1; x >= topLeft.x; x--) seq.push({ x, y: bottomRight.y });
  for (let y = bottomRight.y - 1; y > topLeft.y; y--) seq.push({ x: topLeft.x, y });
  return seq;
}

function generateListeningSequence(rows: number, columns: number): Coordinate[] {
  const center = { x: Math.floor(columns / 2), y: Math.floor(rows / 2) };
  const noIndex = { x: -1, y: -1 };
  return [center, noIndex, noIndex, noIndex, noIndex, noIndex, noIndex, noIndex, noIndex];
}

function generateThinkingSequence(rows: number, columns: number): Coordinate[] {
  const seq: Coordinate[] = [];
  const y = Math.floor(rows / 2);
  for (let x = 0; x < columns; x++) seq.push({ x, y });
  for (let x = columns - 1; x >= 0; x--) seq.push({ x, y });
  return seq;
}

function gridSequence(state: DotGridState, rows: number, columns: number, radius: number): Coordinate[] {
  const clamped = Math.min(radius, Math.floor(Math.max(rows, columns) / 2));
  if (state === "thinking") return generateThinkingSequence(rows, columns);
  if (state === "connecting") return generateConnectingSequence(rows, columns, clamped);
  if (state === "listening") return generateListeningSequence(rows, columns);
  return [{ x: Math.floor(columns / 2), y: Math.floor(rows / 2) }];
}

/** True for the states that step through a sequence on a timer. */
export function sequenceAnimates(state: DotGridState): boolean {
  return state === "thinking" || state === "connecting" || state === "listening";
}

function frameOf(lit: (x: number, y: number) => boolean): boolean[][] {
  return Array.from({ length: DOT_ROWS }, (_, y) => Array.from({ length: DOT_COLS }, (_, x) => lit(x, y)));
}

/** The single lit dot of a sequence state at `step`. */
export function sequenceActive(state: DotGridState, step: number): boolean[][] {
  const seq = gridSequence(state, DOT_ROWS, DOT_COLS, DOT_COLS);
  const dot = seq[step % seq.length] ?? { x: -1, y: -1 };
  return frameOf((x, y) => x === dot.x && y === dot.y);
}

/** A legible still frame for reduced motion: the whole path a sequence
 * would sweep, lit at once (middle row for thinking, the full ring for
 * connecting, the centre dot for listening). One lit dot frozen in place
 * reads as a hang. */
export function staticFrame(state: DotGridState): boolean[][] {
  const seq = gridSequence(state, DOT_ROWS, DOT_COLS, DOT_COLS);
  const lit = new Set(seq.filter((c) => c.x >= 0).map((c) => `${c.x},${c.y}`));
  return frameOf((x, y) => lit.has(`${x},${y}`));
}

// The speaking state: a 2D center-bright falloff. The grid glows from the
// center outward, with the audio level controlling the radius of the bright
// area: a soft rectangular glow, bright center, fading to dim at the edges.
export function speakingIntensity(levels: number[]): number[][] {
  const hist = levels.slice(-DOT_COLS);
  while (hist.length < DOT_COLS) hist.unshift(0);
  const currentLevel = hist[hist.length - 1] ?? 0;
  const colMid = (DOT_COLS - 1) / 2;
  const rowMid = (DOT_ROWS - 1) / 2;

  // The "radius" of the bright zone scales with the audio level. Higher
  // base + multiplier so the glow covers the full 25×7 grid, including the
  // top and bottom rows, at moderate-to-loud speech.
  const radius = 0.45 + currentLevel * 1.1;

  return Array.from({ length: DOT_ROWS }, (_, y) => {
    const rowNorm = Math.abs(y - rowMid) / rowMid; // 0 at center, 1 at edge
    return Array.from({ length: DOT_COLS }, (_, x) => {
      const colNorm = Math.abs(x - colMid) / colMid; // 0 at center, 1 at edge
      // Circular-ish distance from center. With a 25×7 grid the row axis is
      // short, so no vertical compression: the glow reaches the top and
      // bottom rows at moderate speech levels.
      const dist = Math.sqrt(colNorm * colNorm + rowNorm * rowNorm);
      // Outside the glow: very dim (still slightly visible).
      if (dist > radius) return 0.15;
      // Inside: full brightness with a soft edge.
      const edge = radius * 0.8;
      if (dist < edge) return 1;
      return 1 - ((dist - edge) / (radius - edge)) * 0.85;
    });
  });
}
