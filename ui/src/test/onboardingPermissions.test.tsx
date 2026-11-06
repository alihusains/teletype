// claim: ui-behavior
/**
 * The permission step shows live status: a missing permission offers
 * "Open System Settings", flips to "Granted" by itself when the OS grant
 * lands (backend event or the 1 s re-read), and enables Continue with no
 * restart. Repair mode (onboarding done earlier, permission missing) shows only
 * this step plus the "Try it" hotkey check. The launch rule is a pure function.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

describe("TryHotkeyCheck keycap", () => {
  const originalPlatform = Object.getOwnPropertyDescriptor(navigator, "platform");
  const originalUad = Object.getOwnPropertyDescriptor(navigator, "userAgentData");
  const pristinePlatformValue = navigator.platform;

  function setPlatform(value: string) {
    Object.defineProperty(navigator, "platform", { value, configurable: true });
    Object.defineProperty(navigator, "userAgentData", { value: undefined, configurable: true });
  }

  function restoreNavigator() {
    for (const [key, original] of [
      ["platform", originalPlatform],
      ["userAgentData", originalUad],
    ] as const) {
      if (original) Object.defineProperty(navigator, key, original);
      else Reflect.deleteProperty(navigator, key);
    }
  }

  beforeEach(() => {
    setPlatform("MacIntel");
  });

  afterEach(() => {
    restoreNavigator();
  });

  function setupTryIt(hotkey: string) {
    mockListenCapture();
    mockInvokeResponses({
      get_settings: { hotkey, selectedSpeechModel: "x", hasCompletedOnboarding: true },
      get_permissions: [MIC_OK, AX_OK],
      list_speech_models: [],
      list_models: [],
      get_speech_model_ready_state: { id: "x", downloaded: false, ready: false, message: "" },
      open_permission_settings: null,
      request_permission: null,
    });
    render(<OnboardingScreen onCompleted={vi.fn()} repair={true} />);
  }

  async function openTryIt() {
    await screen.findByText("Grant permissions");
    fireEvent.click(await screen.findByText("Continue"));
    return screen.findByTestId("try-hotkey");
  }

  it("renders the Mac Control glyph in a kbd element when Ctrl is configured", async () => {
    setupTryIt("Ctrl");
    const box = await openTryIt();
    const kbd = box.querySelector("kbd");
    expect(kbd).toBeTruthy();
    expect(kbd!.textContent).toContain("\u2303");
    expect(kbd!.textContent).toContain("Control");
  });

  it("renders combo keys as separate kbd caps with the Mac Control glyph", async () => {
    setupTryIt("Ctrl+Shift+A");
    const box = await openTryIt();
    const kbds = box.querySelectorAll("kbd");
    expect(kbds.length).toBe(3);
    expect(kbds[0].textContent).toContain("\u2303");
    expect(kbds[0].textContent).toContain("Control");
    expect(kbds[1].textContent).toContain("\u21E7");
    expect(kbds[2].textContent).toContain("A");
  });

  it("renders Ctrl without the Mac glyph on non-Mac platforms", async () => {
    setPlatform("Win32");
    setupTryIt("Ctrl");
    const box = await openTryIt();
    const kbd = box.querySelector("kbd");
    expect(kbd).toBeTruthy();
    expect(kbd!.textContent).not.toContain("\u2303");
    expect(kbd!.textContent).toContain("Ctrl");
  });

  it("keeps the Control glyph decorative while the readable label stays exposed", async () => {
    setupTryIt("Ctrl");
    const box = await openTryIt();
    const kbd = box.querySelector("kbd") as HTMLElement;
    expect(kbd).toBeTruthy();
    expect(kbd.getAttribute("aria-label")).toBeNull();
    const glyphSpan = kbd.querySelector("span[aria-hidden='true']");
    expect(glyphSpan).toBeTruthy();
    expect(glyphSpan!.textContent).toBe("\u2303");
    expect(kbd.textContent).toContain("Control");
  });

  it("renders Fn as plain text without any glyph on Mac", async () => {
    setupTryIt("Fn");
    const box = await openTryIt();
    const kbd = box.querySelector("kbd") as HTMLElement;
    expect(kbd).toBeTruthy();
    expect(kbd.textContent).toBe("Globe (Fn)");
    expect(kbd.querySelector("span[aria-hidden='true']")).toBeNull();
  });

  it("restores the original navigator descriptors exactly, including absence", async () => {
    // The describe-level beforeEach has already overridden navigator.platform
    // for this test, so the pristine state is the describe-level original.
    const pristinePlatform = originalPlatform ?? undefined;
    const pristineUad = originalUad ?? undefined;
    setPlatform("Win32");
    restoreNavigator();
    expect(Object.getOwnPropertyDescriptor(navigator, "platform")).toEqual(pristinePlatform);
    expect(Object.getOwnPropertyDescriptor(navigator, "userAgentData")).toEqual(pristineUad);
    setPlatform("MacIntel");
    restoreNavigator();
    expect(Object.getOwnPropertyDescriptor(navigator, "platform")).toEqual(pristinePlatform);
    expect(Object.getOwnPropertyDescriptor(navigator, "userAgentData")).toEqual(pristineUad);
  });

  it("does not leak overrides when the original own descriptors were absent", async () => {
    const pristine = originalPlatform ?? undefined;
    const pristineOwn = originalPlatform !== undefined;
    setPlatform("MacIntel");
    expect(navigator.platform).toBe("MacIntel");
    restoreNavigator();
    expect(Object.getOwnPropertyDescriptor(navigator, "platform")).toEqual(pristine);
    expect(Object.prototype.hasOwnProperty.call(navigator, "platform")).toBe(pristineOwn);
    expect(navigator.platform).toBe(pristinePlatformValue);
  });

  it("shows the plain fallback text when no hotkey is configured", async () => {
    setupTryIt("");
    const box = await openTryIt();
    expect(box.querySelector("kbd")).toBeNull();
    expect(box.textContent).toContain("your hotkey");
  });

  it("shows the success message with the Mac Control glyph after dictation-state", async () => {
    setupTryIt("Ctrl");
    const box = await openTryIt();
    expect(box.textContent).toContain("Try it");
    await act(async () => {
      await emitTauriEvent("dictation-state", { phase: "recording" });
    });
    await screen.findByText(/Hotkey works/);
    const kbd = box.querySelector("kbd");
    expect(kbd).toBeTruthy();
    expect(kbd!.textContent).toContain("\u2303");
  });
});
