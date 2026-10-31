// claim: ui-behavior
/**
 * The permission step shows live status: a missing permission offers
 * "Open System Settings", flips to "Granted" by itself when the OS grant
 * lands (backend event or the 1 s re-read), and enables Continue with no
 * restart. Repair mode (onboarding done earlier, permission missing) shows only
 * this step plus the "Try it" hotkey check. The launch rule is a pure function.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import OnboardingScreen from "../screens/OnboardingScreen";
import { needsPermissionStep, shouldShowPermissionRepair } from "../lib/permissions";
import {
  emitTauriEvent,
  invokeCalls,
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
} from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const MIC_OK = { kind: "microphone", state: "granted" } as const;
const AX_MISSING = { kind: "accessibility", state: "denied" } as const;
const AX_OK = { kind: "accessibility", state: "granted" } as const;

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

function setup(permissions: unknown, repair = false) {
  mockListenCapture();
  mockInvokeResponses({
    get_settings: { hotkey: "Fn", selectedSpeechModel: "x", hasCompletedOnboarding: repair },
    get_permissions: permissions,
    list_speech_models: [],
    list_models: [],
    get_speech_model_ready_state: { id: "x", downloaded: false, ready: false, message: "" },
    open_permission_settings: null,
    request_permission: null,
  });
  const done = vi.fn();
  render(<OnboardingScreen onCompleted={done} repair={repair} />);
  return done;
}

async function openPermissionStep() {
  fireEvent.click(await screen.findByText("Continue"));
  await screen.findByText("Grant permissions");
}

describe("permission step", () => {
  it("missing: offers Open System Settings and keeps Continue disabled", async () => {
    setup([MIC_OK, AX_MISSING]);
    await openPermissionStep();
    const row = await screen.findByTestId("permission-accessibility");
    fireEvent.click(row.querySelector("button.primary") as HTMLElement);
    expect(invokeCalls()).toContainEqual({
      cmd: "open_permission_settings",
      args: { kind: "accessibility" },
    });
    expect((screen.getByText("Continue") as HTMLButtonElement).disabled).toBe(true);
  });

  it("granted: shows Granted for each row and enables Continue", async () => {
    setup([MIC_OK, AX_OK]);
    await openPermissionStep();
    await waitFor(() => expect(screen.getAllByText("✓ Granted")).toHaveLength(2));
    expect((screen.getByText("Continue") as HTMLButtonElement).disabled).toBe(false);
  });

  it("live update via the backend event enables Continue without a refresh", async () => {
    setup([MIC_OK, AX_MISSING]);
    await openPermissionStep();
    await screen.findByText("Open System Settings");
    await act(async () => {
      await emitTauriEvent("permissions-changed", [MIC_OK, AX_OK]);
    });
    await waitFor(() => expect(screen.queryByText("Open System Settings")).toBeNull());
    expect((screen.getByText("Continue") as HTMLButtonElement).disabled).toBe(false);
  });

  it("live update via the 1 s re-read when no event arrives", async () => {
    let calls = 0;
    setup(() => (++calls < 3 ? [MIC_OK, AX_MISSING] : [MIC_OK, AX_OK]));
    await openPermissionStep();
    await waitFor(() => expect(screen.getAllByText("✓ Granted")).toHaveLength(2), {
      timeout: 5000,
    });
    expect((screen.getByText("Continue") as HTMLButtonElement).disabled).toBe(false);
  }, 10000);
});

describe("repair mode", () => {
  it("starts on the permission step, then the Try it check, then returns to the app", async () => {
    const done = setup([MIC_OK, AX_OK], true);
    await screen.findByText("Grant permissions");
    expect(screen.queryByText("Back")).toBeNull();
    fireEvent.click(await screen.findByText("Continue"));
    await screen.findByTestId("try-hotkey");
    expect(screen.getByText("Fn")).toBeTruthy();
    await act(async () => {
      await emitTauriEvent("dictation-state", { phase: "recording" });
    });
    await screen.findByText(/Hotkey works/);
    fireEvent.click(screen.getByText("Continue to Teletype"));
    expect(done).toHaveBeenCalled();
  });
});

describe("launch-time rule", () => {
  it("shows the permission step for a finished onboarding only when something is missing", () => {
    expect(shouldShowPermissionRepair(true, [MIC_OK, AX_MISSING])).toBe(true);
    expect(shouldShowPermissionRepair(true, [MIC_OK, AX_OK])).toBe(false);
    expect(shouldShowPermissionRepair(false, [MIC_OK, AX_MISSING])).toBe(false);
  });

  it("treats unknown or unsupported as not missing", () => {
    expect(needsPermissionStep(null)).toBe(false);
    expect(needsPermissionStep([])).toBe(false);
    expect(needsPermissionStep([{ kind: "accessibility", state: "unsupported" }, MIC_OK])).toBe(false);
    expect(needsPermissionStep([{ kind: "microphone", state: "notDetermined" }])).toBe(true);
  });
});
