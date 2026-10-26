import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "./errors";

/**
 * The two things every screen gets wrong in the same way: a read that rejects
 * renders as "nothing yet", and a write that rejects renders as success. Both
 * tell the user something that did not happen.
 *
 * `useResource` separates loading / failed / genuinely empty. `useWrites`
 * carries per-row pending and error state so a screen with forty rows does not
 * grey out all forty when one save is in flight.
 */

export type WriteResult = { ok: true } | { ok: false; error: string };

/** One backend write that cannot be swallowed. Never throws, so the caller can
 *  restore the previous value in the `!ok` branch instead of losing it. */
export async function runWrite(fn: () => unknown): Promise<WriteResult> {
  try {
    await fn();
    return { ok: true };
  } catch (e) {
    return { ok: false, error: errorMessage(e) };
  }
}

export interface Writes {
  /** True while the write keyed by this id is in flight. */
  busy: (key: string) => boolean;
  /** The message from the last failed write for this key, if it is still the
   *  most recent outcome. */
  error: (key: string) => string | null;
  /** Any key currently failed. Screens use this to keep a summary line honest. */
  hasError: boolean;
  run: (key: string, fn: () => unknown) => Promise<WriteResult>;
  clear: (key: string) => void;
}

export function useWrites(): Writes {
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  // The alive flag is per hook instance, not per call: a screen that unmounts
  // mid-write must not write state afterwards.
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const run = useCallback(async (key: string, fn: () => unknown) => {
    setBusy((b) => ({ ...b, [key]: true }));
    setErrors((e) => {
      if (!(key in e)) return e;
      const next = { ...e };
      delete next[key];
      return next;
    });
    const result = await runWrite(fn);
    if (alive.current) {
      setBusy((b) => {
        const next = { ...b };
        delete next[key];
        return next;
      });
      if (!result.ok) {
        setErrors((e) => ({ ...e, [key]: result.error }));
      }
    }
    return result;
  }, []);

  const clear = useCallback((key: string) => {
    setErrors((e) => {
      if (!(key in e)) return e;
      const next = { ...e };
      delete next[key];
      return next;
    });
  }, []);

  return {
    busy: (key) => busy[key] === true,
    error: (key) => errors[key] ?? null,
    hasError: Object.keys(errors).length > 0,
    run,
    clear,
  };
}

export type ResourceStatus = "loading" | "ready" | "error";

export interface Resource<T> {
  data: T | null;
  status: ResourceStatus;
  error: string | null;
  /** Re-read from the backend. Also the "Try again" of a failed load. */
  reload: () => void;
  /** Patch the local copy while a write is in flight. Call `reload` once the
   *  write settles so the screen ends up showing what the disk holds. */
  patch: (updater: (prev: T | null) => T) => void;
}

/**
 * Load once on mount, and re-load when `reload` is called.
 *
 * `loader` is read through a ref, so an inline arrow function is fine and does
 * not re-trigger the effect on every render.
 */
export function useResource<T>(loader: () => Promise<T>): Resource<T> {
  const loaderRef = useRef(loader);
  loaderRef.current = loader;
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [settled, setSettled] = useState(false);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let alive = true;
    loaderRef.current().then(
      (d) => {
        if (!alive) return;
        setData(d);
        setError(null);
        setSettled(true);
      },
      (e) => {
        if (!alive) return;
        setError(errorMessage(e));
        setSettled(true);
      }
    );
    return () => {
      alive = false;
    };
  }, [nonce]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  const patch = useCallback((updater: (prev: T | null) => T) => {
    setData((prev) => updater(prev));
  }, []);

  // The object identity is memoised on purpose. `useResource` is a hook, so a
  // fresh literal every render would make any consumer that lists it in a
  // dependency array re-fire its effect forever: StylesScreen's
  // `useEffect(refresh, [refresh])` called reload() on every pass and spun the
  // renderer while flooding IPC.
  return useMemo(
    () => ({
      data,
      status: !settled ? "loading" : error ? "error" : "ready",
      error,
      reload,
      patch,
    }),
    [data, settled, error, reload, patch]
  );
}
