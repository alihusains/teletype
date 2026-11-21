import { invoke } from "@tauri-apps/api/core";

export interface RewriteWithTransformResult {
  text: string;
  skipMessage: string | null;
}

export async function rewriteWithTransform(text: string): Promise<RewriteWithTransformResult> {
  const result = await invoke<RewriteWithTransformResult>("rewrite_with_transform", { text });
  return {
    text: result?.text ?? "",
    skipMessage: result?.skipMessage ?? null,
  };
}
