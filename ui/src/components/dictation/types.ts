/**
 * Wire types for the Dictation tab. Each interface mirrors a Rust struct the
 * backend serialises (camelCase); `crates/teletype-core/tests/ipc_contract.rs`
 * checks the names and the container shapes against the Rust side.
 */

/** Which stage made a span (`history_edits::SpanKind`). */
export type SpanKind =
  | "same"
  | "filler"
  | "dictionary"
  | "autotext"
  | "polish"
  | "format"
  | "correction";

/** One run of the final text (`history_edits::TextSpan`). */
export interface TextSpan {
  kind: SpanKind;
  /** The words this stage received ("" for a pure insertion). */
  from: string;
  /** New words. Omitted on the wire for `same` spans and removals. */
  to?: string;
  /** Only case and/or punctuation differ. Omitted on the wire when false. */
  minor?: boolean;
  /**
   * A later stage rewrote this change: the span keeps the earlier stage's
   * credit, but its `to` is not in the final text. Omitted when false.
   */
  superseded?: boolean;
}

/** One dictation as `list_dictation_history` returns it (`commands::history::HistoryEntry`). */
export interface HistoryEntry {
  id: string;
  createdAt: number;
  text: string;
  appName: string;
  appType: string;
  rawText: string | null;
  edits: TextSpan[] | null;
  asrModel: string | null;
  polishModel: string | null;
  polishMs: number | null;
  durationMs: number | null;
  wordCount: number;
  hasRecording: boolean;
}

/** Stored recordings (`recordings::RecordingsUsage`). */
export interface RecordingsUsage {
  count: number;
  bytes: number;
}
