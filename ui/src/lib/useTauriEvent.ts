import { emit, listen, type EventCallback } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";

/**
 * Subscribes to a backend event for the lifetime of the component.
 *
 * Uses a `cancelled` flag so that if the component unmounts before the
 * `listen` promise resolves (React StrictMode double-mount), the listener is
 * removed immediately instead of being orphaned or over-unlisted.
 */
export function useTauriEvent<T>(event: string, handler: EventCallback<T>) {
  const latest = useRef(handler);
  latest.current = handler;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<T>(event, (e) => latest.current(e)).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [event]);
}

/**
 * Ask the app shell to show another screen.
 *
 * This is the channel the tray menu already uses: Rust emits `navigate` with a
 * screen id (crates/teletype-desktop/src/tray.rs sends "settings"), and
 * ui/src/App.tsx listens and switches. Emitting it from a screen gives a
 * cross-screen affordance without threading an `onNavigate` prop through every
 * screen, and it degrades to a no-op in the browser preview (the shim has no
 * event backend), so the click is never left hanging.
 */
export function navigateTo(screen: string): void {
  emit("navigate", screen).catch((e) => {
    console.warn(`[teletype] navigate("${screen}") failed:`, e);
  });
}
