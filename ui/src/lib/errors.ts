/// Renders a Tauri invoke failure for the user.
///
/// Backend commands return `CommandError { code, message }` (roadmap S3), so a
/// rejection is an object, not a string — `String(e)` would show
/// "[object Object]". Prefer the structured message, fall back to String().
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e !== null && typeof e === "object" && "message" in e) {
    const m = (e as { message?: unknown }).message;
    if (typeof m === "string" && m.length > 0) return m;
  }
  return String(e);
}

/// The machine-readable error code when the backend sent one, else null.
/// Screens can branch on it (e.g. `E_NOT_FOUND`) instead of matching text.
export function errorCode(e: unknown): string | null {
  if (e !== null && typeof e === "object" && "code" in e) {
    const c = (e as { code?: unknown }).code;
    if (typeof c === "string" && c.length > 0) return c;
  }
  return null;
}
