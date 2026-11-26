// claim: ui-behavior
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { PermissionsSection } from "../settings/PermissionsUpdatesLicenseSections";
import {
  emitTauriEvent,
  invokeCalls,
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
} from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onFocusChanged: () => Promise.resolve(() => {}),
  }),
}));

type SettingsProp = Parameters<typeof PermissionsSection>[0]["settings"];

const PERMISSIONS = [
  { kind: "microphone", state: "granted" },
  { kind: "accessibility", state: "granted" },
];

function mount(status: { active: boolean | null; binding: string; reason: string | null }, reregister?: unknown) {
  mockInvokeResponses({
    get_permissions: PERMISSIONS,
    hotkey_status: status,
    ...(reregister === undefined ? {} : { reregister_hotkey: reregister }),
  });
  render(<PermissionsSection settings={{} as SettingsProp} onChange={() => {}} />);
}

describe("Permissions hotkey row", () => {
  beforeEach(() => {
    resetTauriMocks();
    mockListenCapture();
  });

  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("reads hotkey_status on mount and shows the active chip", async () => {
    mount({ active: true, binding: "Cmd+Shift+Space", reason: null });
    expect(await screen.findByText("Active")).toBeTruthy();
    expect(invokeCalls().some((c) => c.cmd === "hotkey_status")).toBe(true);
    expect(screen.queryByRole("button", { name: "Re-register" })).toBeNull();
  });

  it("shows the inactive chip with a keyboard-accessible re-register button", async () => {
    mount({ active: false, binding: "Fn", reason: null }, null);
    expect(await screen.findByText("Inactive — re-register")).toBeTruthy();
    const button = screen.getByRole("button", { name: "Re-register" });
    expect(button.tagName).toBe("BUTTON");

    fireEvent.click(button);
    await waitFor(() =>
      expect(invokeCalls().some((c) => c.cmd === "reregister_hotkey")).toBe(true),
    );
  });

  it("shows the backend reason when re-registration fails", async () => {
    mount({ active: false, binding: "Fn", reason: null }, null);
    expect(await screen.findByText("Inactive — re-register")).toBeTruthy();
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "reregister_hotkey") throw { message: "hotkey held", code: "E_BUSY" };
      return null;
    });
    fireEvent.click(screen.getByRole("button", { name: "Re-register" }));
    expect((await screen.findByRole("alert")).textContent).toContain("hotkey held");
  });

  it("shows a pushed reason from the hotkey-status event", async () => {
    mount({ active: false, binding: "Fn", reason: null }, null);
    expect(await screen.findByText("Inactive — re-register")).toBeTruthy();
    await act(async () => {
      await emitTauriEvent("hotkey-status", {
        active: false,
        binding: "Fn",
        reason: "registration blocked by accessibility",
      });
    });
    expect((await screen.findByRole("alert")).textContent).toContain("registration blocked by accessibility");
  });

  it("marks a binding with no liveness signal as not checkable", async () => {
    mount({ active: null, binding: "Cmd+Shift+Space", reason: null });
    expect(await screen.findByText("Not checkable for this binding")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Re-register" })).toBeNull();
  });

  it("permission_refresh_follows_focus_not_a_timer", async () => {
    vi.useFakeTimers();
    try {
      mount({ active: true, binding: "Fn", reason: null });
      const permissionCalls = () =>
        invokeCalls().filter((c) => c.cmd === "get_permissions").length;

      await act(async () => {});
      const initial = permissionCalls();
      expect(initial).toBeGreaterThan(0);

      // No timer: half a minute of a visible window must not re-probe.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(30000);
      });
      expect(permissionCalls()).toBe(initial);

      // Coming back from System Settings is what refreshes.
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
      });
      expect(permissionCalls()).toBe(initial + 1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("updates the chip when a fresh hotkey-status event arrives", async () => {
    mount({ active: false, binding: "Fn", reason: null }, null);
    expect(await screen.findByText("Inactive — re-register")).toBeTruthy();
    await act(async () => {
      await emitTauriEvent("hotkey-status", { active: true, binding: "Fn", reason: null });
    });
    expect(await screen.findByText("Active")).toBeTruthy();
    expect(screen.queryByText("Inactive — re-register")).toBeNull();
  });
});
