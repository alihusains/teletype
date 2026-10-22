// Native dialogs and external links, with a browser-preview fallback.
//
// Why this exists: inside this app's WKWebView `window.confirm()` always
// returns false (wry implements no confirm-panel delegate) and `window.open()`
// / `target="_blank"` popups are silently refused. Every destructive gate and
// every outbound link built on those was therefore a dead control that looked
// live. The Tauri dialog and opener plugins are the working path: both
// packages are installed, both Rust plugins are initialised, and the
// capabilities are granted in crates/teletype-desktop/capabilities/default.json
// (`dialog:allow-*`, `opener:default`).
//
// The fallbacks keep the dev browser preview usable: ui/src/dev/browserPreview.ts
// shims `__TAURI_INTERNALS__` with in-memory data only, so it has no dialog or
// opener backend. When a plugin call is unavailable (plain browser preview, an
// older backend, or a denied permission) we degrade to the native function and
// log the reason instead of silently dropping the user's click.

import { confirm, message } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

/** True when running inside a Tauri webview (same guard as browserPreview.ts). */
export function isTauriRuntime(): boolean {
  return (
    (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !==
    undefined
  );
}

/**
 * A yes/no gate. Prefer this over `window.confirm`, which returns false here.
 *
 * `title` sets the window title; `kind` picks the icon (error for destructive).
 */
export async function confirmDialog(
  messageText: string,
  options?: { title?: string; kind?: "info" | "warning" | "error" },
): Promise<boolean> {
  if (isTauriRuntime()) {
    try {
      return await confirm(messageText, {
        title: options?.title,
        kind: options?.kind ?? "warning",
      });
    } catch (e) {
      // Permission denied or an older plugin: fall through to the native
      // dialog so the click still does something in a plain browser.
      console.warn("[teletype] dialog confirm unavailable, using window.confirm:", e);
    }
  }
  return window.confirm(messageText);
}

/**
 * An informational popup. Replaces `window.alert`, which the WKWebView refuses.
 * Resolves once the user dismisses it; callers should not branch on the result.
 */
export async function messageDialog(
  text: string,
  options?: { title?: string; kind?: "info" | "warning" | "error" },
): Promise<void> {
  if (isTauriRuntime()) {
    try {
      await message(text, {
        title: options?.title,
        kind: options?.kind ?? "info",
      });
      return;
    } catch (e) {
      console.warn("[teletype] dialog message unavailable, using window.alert:", e);
    }
  }
  window.alert(text);
}

/**
 * Opens an http(s) link in the user's default browser. Replaces `window.open`
 * and `target="_blank"`, both refused by this webview.
 */
export async function openExternal(url: string): Promise<void> {
  if (isTauriRuntime()) {
    try {
      await openUrl(url);
      return;
    } catch (e) {
      console.warn("[teletype] opener unavailable, using window.open:", e);
    }
  }
  window.open(url, "_blank", "noopener");
}
