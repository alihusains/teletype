import { Fragment } from "react";
import type { SpanKind, TextSpan } from "./types";

/** Visually hidden, still read by screen readers. */
export const srOnly: React.CSSProperties = {
  position: "absolute",
  width: 1,
  height: 1,
  padding: 0,
  margin: -1,
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
  border: 0,
};

/**
 * Text that is on screen for the reader but not part of the dictation: a
 * drag-select and copy must give back only what was pasted.
 */
const noSelect: React.CSSProperties = { userSelect: "none", WebkitUserSelect: "none" };

const delStyle: React.CSSProperties = {
  ...noSelect,
  textDecoration: "line-through",
  color: "var(--text-tertiary)",
};

const insStyle: React.CSSProperties = {
  textDecoration: "none",
  background: "var(--accent-soft)",
  color: "var(--accent)",
  borderRadius: 4,
  padding: "0 2px",
};

/** True when the span changed something worth marking. */
export function isMarked(span: TextSpan): boolean {
  return span.kind !== "same" && !span.minor;
}

/** What a changed span did, for its tooltip and the screen-reader prefix. */
export function describeSpan(span: TextSpan): string {
  const what = describeChange(span);
  return span.superseded && what ? `${what}, then changed again` : what;
}

function describeChange(span: TextSpan): string {
  const from = span.from;
  const to = span.to ?? "";
  switch (span.kind) {
    case "filler":
      return to ? `Filler fixed: ${from} to ${to}` : `Filler removed: ${from}`;
    case "polish":
      return from && to ? `Polish changed ${from} to ${to}` : from ? `Polish removed ${from}` : `Polish added ${to}`;
    case "dictionary":
      return from && to
        ? `Dictionary fixed ${from} to ${to}`
        : from
          ? `Dictionary removed ${from}`
          : `Dictionary added ${to}`;
    case "autotext":
      return "AutoText expanded";
    case "format":
      return "Formatted";
    case "correction":
      return "Matched on-screen name";
    default:
      return "";
  }
}

/** The words a span puts in the final text. */
function visibleWords(span: TextSpan): string[] {
  if (span.superseded) return [];
  const shown = span.kind === "same" ? span.from : (span.to ?? "");
  return shown.split(/\s+/).filter(Boolean);
}

/** The final text as words, each with the exact whitespace before it. */
function wordsWithSeparators(text: string): { sep: string; word: string }[] {
  return Array.from(text.matchAll(/(\s*)(\S+)/g), (m) => ({ sep: m[1], word: m[2] }));
}

/**
 * The final text with each stage's change shown inline: removals struck
 * through, additions highlighted. With `showChanges` off (or no edits) it is
 * the plain final text.
 *
 * The spans carry words only, so the visible text is sliced from `text`
 * itself: newlines and blank lines the polish pass wrote survive. Spans that
 * do not spell `text` fall back to the plain text rather than show a card
 * that differs from what was pasted.
 */
export function InlineDiff({
  text,
  edits,
  showChanges,
}: {
  text: string;
  edits: TextSpan[] | null;
  showChanges: boolean;
}) {
  if (!showChanges || !edits || edits.length === 0) return <>{text}</>;
  const words = wordsWithSeparators(text);
  const spelled = edits.flatMap(visibleWords);
  if (spelled.length !== words.length || spelled.some((w, i) => w !== words[i].word)) {
    return <>{text}</>;
  }
  let cursor = 0;
  let started = false;
  return (
    <>
      {edits.map((span, i) => {
        const count = visibleWords(span).length;
        // The span's slice of the final text, with its own whitespace.
        const slice = words
          .slice(cursor, cursor + count)
          .map((w, j) => (j === 0 ? w.word : w.sep + w.word))
          .join("");
        const sep = !started ? "" : count > 0 ? words[cursor].sep : " ";
        cursor += count;
        if (span.kind === "same") {
          started = true;
          return (
            <Fragment key={i}>
              {sep}
              {slice}
            </Fragment>
          );
        }
        if (span.minor) {
          // A case or punctuation fix: only the new word, undecorated, so
          // capitalisation does not cover the card in highlights. A minor
          // change a later stage rewrote shows nothing at all.
          if (!slice) return null;
          started = true;
          return (
            <Fragment key={i}>
              {sep}
              {slice}
            </Fragment>
          );
        }
        started = true;
        const label = describeSpan(span);
        return (
          <Fragment key={i}>
            {/* A removal adds no words, so its space is not copied either. */}
            {count > 0 ? sep : sep && <span style={noSelect}>{sep}</span>}
            <span title={label} data-kind={span.kind} data-superseded={span.superseded ? "" : undefined}>
              <span style={{ ...srOnly, ...noSelect }}>{label}. </span>
              {span.from && (
                <del style={delStyle} aria-hidden="true">
                  {span.from}
                </del>
              )}
              {span.from && slice ? <span style={noSelect}> </span> : ""}
              {slice && <ins style={insStyle}>{slice}</ins>}
            </span>
          </Fragment>
        );
      })}
    </>
  );
}

/** "2 fillers removed · 3 polish edits · 1 dictionary fix · polished in 0.4s". */
export function changeSummary(edits: TextSpan[] | null, polishMs: number | null): string | null {
  if (!edits) return null;
  const counts: Partial<Record<SpanKind, number>> = {};
  for (const s of edits) {
    if (!isMarked(s)) continue;
    counts[s.kind] = (counts[s.kind] ?? 0) + Math.max(1, wordCountOf(s));
  }
  const n = (k: SpanKind) => counts[k] ?? 0;
  const plural = (c: number, one: string, many: string) => `${c} ${c === 1 ? one : many}`;
  const parts: string[] = [];
  if (n("filler")) parts.push(plural(n("filler"), "filler removed", "fillers removed"));
  if (n("polish")) parts.push(plural(n("polish"), "polish edit", "polish edits"));
  if (n("dictionary")) parts.push(plural(n("dictionary"), "dictionary fix", "dictionary fixes"));
  if (n("autotext")) parts.push(plural(n("autotext"), "AutoText expansion", "AutoText expansions"));
  if (n("format")) parts.push(plural(n("format"), "format fix", "format fixes"));
  if (n("correction")) parts.push(plural(n("correction"), "on-screen name", "on-screen names"));
  if (parts.length === 0) return null;
  if (polishMs !== null && n("polish") > 0) parts.push(`polished in ${(polishMs / 1000).toFixed(1)}s`);
  return parts.join(" · ");
}

/**
 * How many changes a span stands for. A filler span merges adjacent removed
 * words ("um uh"), so it counts each word; other kinds count once per span.
 */
function wordCountOf(span: TextSpan): number {
  if (span.kind !== "filler") return 1;
  return span.from.split(/\s+/).filter(Boolean).length;
}
