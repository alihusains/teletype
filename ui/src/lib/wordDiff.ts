/** Word-level diff for the post-insertion polish card.
 *
 * The card is small and short-lived, so this is deliberately a plain LCS over
 * whitespace tokens rather than a character diff: word granularity is what a
 * reader can scan in a couple of lines, and dictation takes are short.
 */

export type DiffKind = "same" | "add" | "del";

export interface DiffSegment {
  kind: DiffKind;
  text: string;
}

/** Token counts above this fall back to a coarse before/after pair. */
const MAX_TOKENS = 400;

function tokenize(text: string): string[] {
  return text.trim().split(/\s+/).filter(Boolean);
}

function coarse(before: string, after: string): DiffSegment[] {
  const out: DiffSegment[] = [];
  if (before.trim()) out.push({ kind: "del", text: before.trim() });
  if (after.trim()) out.push({ kind: "add", text: after.trim() });
  return out;
}

/** Diff two texts word by word, merging consecutive same-kind runs. */
export function diffWords(before: string, after: string): DiffSegment[] {
  if (before === after) return before.trim() ? [{ kind: "same", text: before.trim() }] : [];
  const a = tokenize(before);
  const b = tokenize(after);
  if (a.length === 0) return b.length ? [{ kind: "add", text: after.trim() }] : [];
  if (b.length === 0) return [{ kind: "del", text: before.trim() }];
  if (a.length > MAX_TOKENS || b.length > MAX_TOKENS) return coarse(before, after);

  // lcs[i][j] = LCS length of a[i..] and b[j..]
  const width = b.length + 1;
  const lcs = new Uint32Array((a.length + 1) * width);
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i * width + j] =
        a[i] === b[j]
          ? lcs[(i + 1) * width + j + 1] + 1
          : Math.max(lcs[(i + 1) * width + j], lcs[i * width + j + 1]);
    }
  }

  const raw: DiffSegment[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      raw.push({ kind: "same", text: a[i] });
      i++;
      j++;
    } else if (lcs[(i + 1) * width + j] >= lcs[i * width + j + 1]) {
      raw.push({ kind: "del", text: a[i] });
      i++;
    } else {
      raw.push({ kind: "add", text: b[j] });
      j++;
    }
  }
  while (i < a.length) raw.push({ kind: "del", text: a[i++] });
  while (j < b.length) raw.push({ kind: "add", text: b[j++] });

  // Merge adjacent runs so "every word changed" renders as one span, not one
  // span per word.
  const merged: DiffSegment[] = [];
  for (const seg of raw) {
    const last = merged[merged.length - 1];
    if (last && last.kind === seg.kind) last.text += ` ${seg.text}`;
    else merged.push({ ...seg });
  }
  return merged;
}

/** Number of changed places. A deletion immediately followed by an insertion
 * is one change, not two, which is how a reader counts them. */
export function changeCount(segments: DiffSegment[]): number {
  let count = 0;
  let inside = false;
  for (const seg of segments) {
    if (seg.kind === "same") inside = false;
    else if (!inside) {
      count++;
      inside = true;
    }
  }
  return count;
}

/** "0.84" -> "0.8s", "1640" -> "1.6s". */
export function formatPolishMs(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "";
  return `${(ms / 1000).toFixed(1)}s`;
}
