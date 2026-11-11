// claim: ui-behavior
/**
 * The onboarding Dictation step used to dead-end when the model list read
 * failed: no card, no error, and a permanently disabled Continue. An empty
 * shortlist is a failed read (the backend always includes the recommended
 * row), so the step must say so with a retry.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import OnboardingScreen from "../screens/OnboardingScreen";
import { invokeCalls, mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

function setup(listSpeechModels: unknown) {
  mockListenCapture();
  mockInvokeResponses({
    get_settings: { hotkey: "Fn", selectedSpeechModel: "x", hasCompletedOnboarding: false },
    get_permissions: [
      { kind: "microphone", state: "granted" },
      { kind: "accessibility", state: "granted" },
    ],
    list_speech_models: listSpeechModels,
    list_models: [],
    get_speech_model_ready_state: { id: "x", downloaded: false, ready: false, message: "" },
  });
  render(<OnboardingScreen onCompleted={vi.fn()} />);
}

async function toDictationStep() {
  // Privacy, then Permissions: the permission step's Continue is enabled only
  // once the live permission read lands, and a click on a detached node (the
  // previous step's button) is lost, so wait between steps.
  fireEvent.click(await screen.findByText("Continue"));
  await screen.findByText("Grant permissions");
  await waitFor(() =>
    expect((screen.getByText("Continue") as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(screen.getByText("Continue"));
  await screen.findByText("Set up your dictation model");
}

describe("onboarding dictation model step", () => {
  it("shows a retryable error when the model list read rejects", async () => {
    setup(() => Promise.reject(new Error("models dir locked")));
    await toDictationStep();

    await screen.findByText("the dictation models could not be read.");
    expect(screen.getByText("models dir locked")).toBeTruthy();

    // Continue stays disabled, but the user can see why and retry.
    expect((screen.getByText("Continue") as HTMLButtonElement).disabled).toBe(true);

    const before = invokeCalls().filter((c) => c.cmd === "list_speech_models").length;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(invokeCalls().filter((c) => c.cmd === "list_speech_models").length).toBeGreaterThan(before);
    });
  });

  it("offers a retry when the read succeeds but comes back empty", async () => {
    setup([]);
    await toDictationStep();

    await screen.findByText("the dictation models could not be read.");
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
    expect((screen.getByText("Continue") as HTMLButtonElement).disabled).toBe(true);
  });
});
