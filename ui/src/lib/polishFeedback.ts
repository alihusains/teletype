import { invoke } from "@tauri-apps/api/core";

export type PolishFeedbackKind =
  | "thumb_up"
  | "thumb_down"
  | "never_change"
  | "undo"
  | "copy_raw";

export interface PolishFeedbackSpan {
  original: string;
  replacement: string;
}

export interface PolishFeedbackInput {
  takeId: number;
  kind: PolishFeedbackKind;
  term?: string | null;
  span?: PolishFeedbackSpan | null;
  /** Honest nulls: the result card does not yet carry app metadata. */
  appId?: string | null;
  model?: string | null;
  /** Polish profile from the result card, forwarded for bandit attribution. */
  profile?: string | null;
}

/** Records local-only feedback. The backend validates and timestamps it. */
export async function sendPolishFeedback(input: PolishFeedbackInput): Promise<void> {
  await invoke("record_polish_feedback", {
    feedback: {
      takeId: input.takeId,
      kind: input.kind,
      term: input.term ?? null,
      span: input.span ?? null,
      appId: input.appId ?? null,
      model: input.model ?? null,
      profile: input.profile ?? null,
    },
  });
}
