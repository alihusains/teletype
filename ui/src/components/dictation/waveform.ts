/**
 * Waveform peaks for the Dictation tab's player, from the raw WAV bytes
 * `get_dictation_audio` returns. Pure functions, no DOM.
 */

/** Bars drawn per recording. */
export const BAR_COUNT = 96;

export interface Waveform {
  /** One value per bar, 0..1, normalised to the take's loudest bar. */
  peaks: number[];
  /** Total length in seconds. */
  durationSec: number;
}

/** The 16-bit PCM payload of a RIFF/WAVE file, or null when it is not one. */
export function parseWav(buf: ArrayBuffer): { samples: Int16Array; sampleRate: number } | null {
  const view = new DataView(buf);
  const tag = (at: number) =>
    String.fromCharCode(view.getUint8(at), view.getUint8(at + 1), view.getUint8(at + 2), view.getUint8(at + 3));
  if (buf.byteLength < 12 || tag(0) !== "RIFF" || tag(8) !== "WAVE") return null;
  let sampleRate = 16000;
  let bits = 16;
  // Walk the chunks: the data chunk is not always at byte 36 (LIST and
  // other chunks may come first).
  let at = 12;
  while (at + 8 <= buf.byteLength) {
    const id = tag(at);
    const size = view.getUint32(at + 4, true);
    const body = at + 8;
    if (id === "fmt " && size >= 16 && body + 16 <= buf.byteLength) {
      sampleRate = view.getUint32(body + 4, true);
      bits = view.getUint16(body + 14, true);
    } else if (id === "data") {
      if (bits !== 16) return null;
      const len = Math.min(size, buf.byteLength - body);
      const count = Math.floor(len / 2);
      const samples = new Int16Array(count);
      for (let i = 0; i < count; i++) samples[i] = view.getInt16(body + i * 2, true);
      return { samples, sampleRate };
    }
    // Chunks are padded to an even size.
    at = body + size + (size % 2);
  }
  return null;
}

/**
 * Splits the samples into `bars` buckets and takes each bucket's RMS,
 * normalised to the loudest bucket, then a square-root curve so quiet
 * speech still reads as speech.
 */
export function peaksFromSamples(samples: Int16Array, bars: number = BAR_COUNT): number[] {
  const out = new Array<number>(bars).fill(0);
  if (samples.length === 0) return out;
  for (let b = 0; b < bars; b++) {
    const start = Math.floor((b * samples.length) / bars);
    const end = Math.max(start + 1, Math.floor(((b + 1) * samples.length) / bars));
    let sum = 0;
    for (let i = start; i < end && i < samples.length; i++) {
      const v = samples[i] / 32768;
      sum += v * v;
    }
    out[b] = Math.sqrt(sum / (end - start));
  }
  const max = Math.max(...out);
  if (max <= 0) return out;
  return out.map((v) => Math.sqrt(v / max));
}

/** Narrowest bar plus the gap after it, in px: the most bars a row can hold. */
export const BAR_PITCH = 4;

/** How many bars fit in `width` px, between 8 and `BAR_COUNT`. */
export function barsForWidth(width: number): number {
  if (!Number.isFinite(width) || width <= 0) return BAR_COUNT;
  return Math.max(8, Math.min(BAR_COUNT, Math.floor((width + 2) / BAR_PITCH)));
}

/** Folds `peaks` into `bars` buckets, keeping each bucket's loudest bar. */
export function resamplePeaks(peaks: number[], bars: number): number[] {
  if (bars >= peaks.length) return peaks;
  const out = new Array<number>(bars).fill(0);
  for (let b = 0; b < bars; b++) {
    const start = Math.floor((b * peaks.length) / bars);
    const end = Math.max(start + 1, Math.floor(((b + 1) * peaks.length) / bars));
    for (let i = start; i < end; i++) out[b] = Math.max(out[b], peaks[i]);
  }
  return out;
}

/** Peaks and duration for a WAV, or null when the bytes are not a 16-bit WAV. */
export function waveformFromWav(buf: ArrayBuffer, bars: number = BAR_COUNT): Waveform | null {
  const wav = parseWav(buf);
  if (!wav) return null;
  return {
    peaks: peaksFromSamples(wav.samples, bars),
    durationSec: wav.samples.length / wav.sampleRate,
  };
}

/** "0:07", "1:05". */
export function clockLabel(sec: number): string {
  const s = Math.max(0, Math.floor(Number.isFinite(sec) ? sec : 0));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Builds a 16 kHz mono 16-bit WAV. Used by tests and the browser preview. */
export function makeWav(samples: ArrayLike<number>, sampleRate = 16000, extraChunk = false): ArrayBuffer {
  const extra = extraChunk ? 8 + 4 : 0;
  const buf = new ArrayBuffer(44 + extra + samples.length * 2);
  const v = new DataView(buf);
  const put = (at: number, s: string) => {
    for (let i = 0; i < 4; i++) v.setUint8(at + i, s.charCodeAt(i));
  };
  put(0, "RIFF");
  v.setUint32(4, buf.byteLength - 8, true);
  put(8, "WAVE");
  put(12, "fmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, sampleRate, true);
  v.setUint32(28, sampleRate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  let at = 36;
  if (extraChunk) {
    put(at, "LIST");
    v.setUint32(at + 4, 4, true);
    put(at + 8, "INFO");
    at += 12;
  }
  put(at, "data");
  v.setUint32(at + 4, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    v.setInt16(at + 8 + i * 2, Math.round(s * 32767), true);
  }
  return buf;
}
