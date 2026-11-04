// A registry of mounted pill roots, for tests.
//
// The list lives on `globalThis` rather than in this module's own state because
// the tests call `vi.resetModules()` before re-importing the pill entry point,
// and a reset discards the previous module instance along with anything it
// held. The roots that instance mounted stay live, so the registry has to
// outlive the module that populated it.

type Unmount = () => void;

declare global {
  // `var` so this is a genuine global binding rather than a module-scoped one.
  var __teletypePillUnmounts: Unmount[] | undefined;
}

export function trackPillRoot(unmount: Unmount): void {
  (globalThis.__teletypePillUnmounts ??= []).push(unmount);
}

/**
 * Unmounts every pill root tracked since the last call.
 *
 * A test that drives the pill through `ui/pillEntry.tsx` leaves a real React
 * root that `@testing-library/react`'s `cleanup()` cannot reach, because cleanup
 * only unmounts roots it created itself. In the recording phase the pill runs a
 * live interval to keep the elapsed time current, and when jsdom is torn down at
 * the end of the test file that interval fires into a React state update that
 * reads `window`. Vitest reports it as an unhandled error and the UI job fails
 * with every test in the file still green. Unmounting in an `afterEach` stops
 * the interval with the component that owns it.
 */
export function unmountAllPillRoots(): void {
  const unmounts = globalThis.__teletypePillUnmounts ?? [];
  globalThis.__teletypePillUnmounts = [];
  for (const unmount of unmounts) unmount();
}
