import { vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";

/**
 * Programmable mock for `@tauri-apps/api/core` `invoke`.
 *
 * Per-test usage:
 *   mockInvokeResponses({ get_logs: [...], get_settings: {...} });
 *   ... render ...
 *   expect(invokeCalls()).toContainEqual({ cmd: "get_logs", args: undefined });
 *
 * Any command without a programmed response rejects, so tests fail loudly
 * instead of hanging on an unresolved backend call.
 */
const invokeResponses = new Map<string, unknown>();

export function mockInvokeResponses(responses: Record<string, unknown>): void {
  invokeResponses.clear();
  for (const [cmd, value] of Object.entries(responses)) {
    invokeResponses.set(cmd, value);
  }
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (invokeResponses.has(cmd)) return invokeResponses.get(cmd);
    throw new Error(`[tauriMock] unexpected invoke: ${cmd}`);
  });
}

export interface InvokeCall {
  cmd: string;
  args?: unknown;
}

export function invokeCalls(): InvokeCall[] {
  return vi.mocked(invoke).mock.calls.map(([cmd, args]) => ({
    cmd: cmd as string,
    args: args as unknown,
  }));
}

/**
 * Capturable mock for `@tauri-apps/api/event` `listen`.
 *
 * Handlers are stored per event name so tests can assert subscription and
 * drive backend pushes:
 *   mockListenCapture();
 *   ... render ...
 *   expect(listenHandlersFor("settings-changed")).toHaveLength(1);
 *   await emitTauriEvent("settings-changed", undefined);
 */
const listenHandlers = new Map<string, Array<EventCallback<never>>>();

export function mockListenCapture(): void {
  listenHandlers.clear();
  vi.mocked(listen).mockImplementation(async (event: string, handler) => {
    const key = event as string;
    const list = listenHandlers.get(key) ?? [];
    const cb = handler as EventCallback<never>;
    list.push(cb);
    listenHandlers.set(key, list);
    return () => {
      listenHandlers.set(
        key,
        (listenHandlers.get(key) ?? []).filter((h) => h !== cb)
      );
    };
  });
}

export function listenHandlersFor<T>(event: string): Array<EventCallback<T>> {
  return ((listenHandlers.get(event) ?? []) as Array<EventCallback<T>>).slice();
}

export async function emitTauriEvent<T>(event: string, payload: T): Promise<void> {
  for (const handler of listenHandlersFor<T>(event)) {
    handler({ event: event as never, id: 0, payload });
  }
}

export function resetTauriMocks(): void {
  invokeResponses.clear();
  listenHandlers.clear();
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockReset();
}
