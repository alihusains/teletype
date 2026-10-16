// Fuzzy search over settings row labels (P8.7, UI only).
//
// Pure and dependency-free so it is trivially unit-testable:
//
//   search(["Record hotkey", "Recording mode"], "recd")
//     -> [{ label: "Record hotkey", score: ... }, ...]
//
// Matching model
// --------------
// A label matches when every character of the query appears in the label in
// order (case-insensitive subsequence). Diacritics are folded on both sides
// before comparing, so "accent" still finds "Accents" and "cafe" finds "café".
//
// Scoring model
// -------------
// Higher is better. The score is a sum of per-character weights:
//
//   +10  the query char starts a word in the label ("r" in "Record")
//   +5   the query char continues a contiguous run (the previous query char
//        matched the immediately preceding label char)
//   +1   any other subsequence hit
//   -1   penalty per label char skipped between two query chars (gaps make a
//        match weaker; the penalty grows with the gap length)
//
// A contiguous run therefore scores far above scattered letters, and a run
// that starts at a word boundary scores highest of all. "rec" in "Recording"
// (10+5+5) beats "rec" in "record hotkey" (10+5+5, same) but both beat
// "r...c...e" scattered across a long label.

export interface Match {
  label: string;
  score: number;
}

// Strip diacritics (é -> e) and lowercase. Both query and label go through
// this, so the match is tolerant of accents without depending on locale.
function fold(s: string): string {
  return s
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase();
}

// Score `query` against `label`. Returns null when the label does not
// contain the query as a subsequence.
function scoreLabel(label: string, query: string): number | null {
  const l = fold(label);
  const q = fold(query);
  if (q.length === 0) return null;

  let score = 0;
  let li = 0; // next label index to consider
  let prevMatched = -1; // label index of the previous query char, or -1
  for (const qc of q) {
    // Word-start: first char of the label, or the char after whitespace.
    const startsWord = (i: number) => i === 0 || /\s/.test(l[i - 1]);
    let found = -1;
    for (let i = li; i < l.length; i++) {
      if (l[i] === qc) {
        found = i;
        break;
      }
    }
    if (found === -1) return null;
    if (startsWord(found)) score += 10;
    else if (found === prevMatched + 1) score += 5;
    else score += 1;
    if (prevMatched !== -1) score -= found - prevMatched - 1;
    li = found + 1;
    prevMatched = found;
  }
  return score;
}

// Search `labels` for `query`; returns the matching labels best-match first.
// An empty (or whitespace-only) query matches nothing, so the caller can
// treat "no query" as "no visual change".
export function search(labels: string[], query: string): Match[] {
  const q = fold(query).trim();
  if (q.length === 0) return [];
  const out: Match[] = [];
  for (const label of labels) {
    const score = scoreLabel(label, q);
    if (score !== null) out.push({ label, score });
  }
  out.sort((a, b) => b.score - a.score || a.label.localeCompare(b.label));
  return out;
}
