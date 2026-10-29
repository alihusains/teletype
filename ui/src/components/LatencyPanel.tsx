/**
 * The per-stage latency panel: p50/p95 per pipeline stage over the most
 * recent takes, the honest engineering metric.
 *
 * Which stages appear and what they are called both come from the backend's
 * `LatencyStageSummary` rows, filtered on `userFacing`; the order comes from
 * each row's `order`, which is pipeline order (not declaration order). A
 * stage with no samples renders a plain hyphen rather than a zero: zero
 * latency is a claim, and "not measured yet" is the truth.
 *
 * The budget promise and the table measure different things, and the panel
 * keeps them apart. The table's "Release to paste" row is every take in the
 * window at every length, on every engine; the 700 ms budget was set against
 * the short clips of one model in the eval corpus. Comparing one to the other
 * reports a violation that is really a length effect or an engine change, so
 * the verdict is drawn from the backend's short-take figure and stated in the
 * sentence under the table, not as a color on a row that carries a different
 * number. That sentence names the engine the figure was measured on, because
 * the same takes ran 892 ms p95 on one model and 1618 ms on another.
 *
 * Drawn as a quiet label/value grid, not a table with rules and a bold
 * header: the column names are caption-size tertiary text and the only
 * hairline is between rows. A header that competes with its own numbers is
 * chart chrome wearing a different hat, and the numbers are the reason the
 * panel exists.
 */

export interface LatencyStageSummary {
  /** Stable stage id (e.g. "capture", "asr", "polish", "inject", "total"). */
  stage: string;
  /** Display label (e.g. "Transcribe", "Release to paste"). */
  label: string;
  /** Pipeline order, used to sort the rows. */
  order: number;
  /** Whether the stage is shown (diagnostic stages are recorded, not shown). */
  userFacing: boolean;
  /** Nearest-rank p50 in ms; null when the stage has no samples. */
  p50Ms: number | null;
  /** Nearest-rank p95 in ms; null when the stage has no samples. */
  p95Ms: number | null;
  /** How many recent takes measured this stage. */
  sampleCount: number;
}

export interface LatencySummary {
  stages: LatencyStageSummary[];
  /** How many recent takes the percentiles were computed over. */
  window: number;
  /** The release-to-paste p95 budget in ms (the promise for the total). */
  releaseToPasteP95MsMax: number;
  /**
   * Release-to-paste p95 over the short takes only, which is the only
   * condition the budget was ever measured under; null when the window has
   * none. The all-lengths figure is not comparable to the promise.
   */
  shortTakeP95Ms: number | null;
  /** How many takes produced `shortTakeP95Ms`, under the engine scope used. */
  shortTakeCount: number;
  /** The audio length under which a take counts as short, in ms. */
  shortTakeAudioMsMax: number;
  /**
   * The engine `shortTakeP95Ms` was measured on, or null when it spans every
   * engine in the window. A p95 is a claim about one engine: the same takes
   * measured 892 ms on one model and 1637 ms on another.
   */
  shortTakeEngine: string | null;
}

const NUMERIC: React.CSSProperties = {
  fontVariantNumeric: "tabular-nums",
  whiteSpace: "nowrap",
};

/** Format a millisecond figure; an unmeasured stage is a hyphen, never 0. */
function fmtMs(ms: number | null): string {
  return ms == null ? "-" : `${ms.toLocaleString()} ms`;
}

/**
 * A bare hyphen is not self-describing, and `aria-label` on a plain span is
 * ignored by most screen readers. `role="img"` makes the label stick, so the
 * cell reads as "not measured" instead of "dash" or nothing.
 */
function msA11y(ms: number | null): React.HTMLAttributes<HTMLSpanElement> | undefined {
  return ms == null ? { role: "img", "aria-label": "not measured" } : undefined;
}

export function LatencyPanel({ summary }: { summary: LatencySummary | null }) {
  // User-facing stages only, in pipeline order.
  const rows = (summary?.stages ?? [])
    .filter((s) => s.userFacing)
    .sort((a, b) => a.order - b.order);

  // Nothing measured yet: say so, rather than drawing a wall of hyphens. An
  // empty `rows` (a failed fetch) lands here too; an empty card under a
  // "Latency" heading reads as a broken feature, and the truth in both cases
  // is the same sentence.
  const measured = rows.some((s) => s.sampleCount > 0);
  if (!measured) {
    return (
      <p style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", margin: 0, minWidth: 0, overflowWrap: "anywhere" }}>
        No timings yet. Latency is measured from your own dictations and appears here after the first one.
      </p>
    );
  }

  const total = rows.find((s) => s.stage === "total");
  const budget = summary?.releaseToPasteP95MsMax ?? 0;
  const shortP95 = summary?.shortTakeP95Ms ?? null;
  const shortCount = summary?.shortTakeCount ?? 0;
  const shortMaxSec = (summary?.shortTakeAudioMsMax ?? 0) / 1000;
  const shortEngine = summary?.shortTakeEngine ?? null;
  const scopeNote = shortEngine ? `on ${shortEngine}` : "across all engines";
  // The verdict belongs to the short-take figure. The table's total row is
  // every length, so it is never tinted against a short-clip budget.
  const over = shortP95 != null && shortP95 > budget;

  return (
    <div style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
      {/* Column captions: caption-size tertiary, right-aligned to the values. */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0, 1fr) 72px 72px 56px",
          gap: "0 10px",
          alignItems: "baseline",
          padding: "0 0 6px",
          minWidth: 0,
        }}
      >
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)" }}>Stage</span>
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", textAlign: "right" }}>p50</span>
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", textAlign: "right" }}>p95</span>
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", textAlign: "right" }}>Takes</span>
      </div>

      {rows.map((row) => {
        const isTotal = row.stage === "total";
        return (
          <div
            key={row.stage}
            title={
              isTotal
                ? `Every take in the window, at every length. The ${budget} ms budget covers takes under ${shortMaxSec} seconds of audio; that figure is in the line below.`
                : `${row.label}: ${row.sampleCount} recent ${row.sampleCount === 1 ? "take" : "takes"}.`
            }
            style={{
              display: "grid",
              gridTemplateColumns: "minmax(0, 1fr) 72px 72px 56px",
              gap: "0 10px",
              alignItems: "baseline",
              padding: "6px 0",
              borderTop: "1px solid var(--border-subtle)",
              minWidth: 0,
            }}
          >
            <span
              style={{
                fontSize: "var(--text-sm)",
                color: "var(--text-secondary)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                minWidth: 0,
              }}
            >
              {row.label}
            </span>
            <span style={{ fontSize: "var(--text-sm)", color: "var(--text)", textAlign: "right", ...NUMERIC }} {...msA11y(row.p50Ms)}>
              {fmtMs(row.p50Ms)}
            </span>
            <span
              style={{
                fontSize: "var(--text-sm)",
                color: "var(--text)",
                textAlign: "right",
                ...NUMERIC,
              }}
              {...msA11y(row.p95Ms)}
            >
              {fmtMs(row.p95Ms)}
            </span>
            <span style={{ fontSize: "var(--text-xs)", color: "var(--text-tertiary)", textAlign: "right", ...NUMERIC }}>
              {row.sampleCount}
            </span>
          </div>
        );
      })}

      {/* The promise, kept next to the figure that actually answers it: short
          takes, and the engine the newest one of them ran on. */}
      {total && shortP95 != null && (
        <p
          style={{
            margin: "8px 0 0",
            fontSize: "var(--text-xs)",
            lineHeight: 1.5,
            minWidth: 0,
            overflowWrap: "anywhere",
            display: "flex",
            alignItems: "baseline",
            gap: 8,
            flexWrap: "wrap",
          }}
        >
          <span style={{ color: over ? "var(--warning)" : "var(--success)", fontWeight: 650, ...NUMERIC }}>
            {shortP95.toLocaleString()} ms
          </span>
          <span style={{ color: "var(--text-tertiary)" }}>
            p95 to paste, {scopeNote}, takes under {shortMaxSec} seconds ({shortCount}{" "}
            {shortCount === 1 ? "take" : "takes"}). {over ? "Over" : "Within"} the {budget} ms budget.
          </span>
        </p>
      )}

      {total && (
        <p style={{ margin: "6px 0 0", fontSize: "var(--text-xs)", color: "var(--text-tertiary)", lineHeight: 1.5, minWidth: 0, overflowWrap: "anywhere" }}>
          {shortP95 == null
            ? `The ${budget} ms budget covers takes under ${shortMaxSec} seconds of audio, and none of the recent takes were that short. `
            : ""}
          The table counts every take at every length, on every engine; a longer
          dictation takes longer to transcribe, and so does a slower model.
          Percentiles cover the most recent {summary?.window ?? 0} takes.
        </p>
      )}
    </div>
  );
}
