export const RAINBOW = [
  "#ff2a40", "#ff8c00", "#ffd700", "#adff2f", "#00fa9a",
  "#00ffff", "#1e90ff", "#4169e1", "#8a2be2",
] as const;

export function rainbowColor(t: number): string {
  const clamped = Math.min(1, Math.max(0, t));
  const scaled = clamped * (RAINBOW.length - 1);
  const i = Math.min(RAINBOW.length - 2, Math.floor(scaled));
  const f = scaled - i;
  const a = RAINBOW[i].match(/\w\w/g)!.map((h) => parseInt(h, 16));
  const b = RAINBOW[i + 1].match(/\w\w/g)!.map((h) => parseInt(h, 16));
  const rgb = a.map((v, k) => Math.round(v + (b[k] - v) * f));
  return `rgb(${rgb[0]},${rgb[1]},${rgb[2]})`;
}

export const SIZES = {
  idle: { width: 58, height: 24 },
  warming: { width: 320, height: 44 },
  processing: { width: 220, height: 44 },
  processingSkip: { width: 420, height: 44 },
  recording: { width: 250, height: 44 },
  recordingExpanded: { width: 460, height: 44 },
} as const;
