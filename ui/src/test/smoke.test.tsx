import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import DeveloperScreen from "../screens/DeveloperScreen";
import {
  emitTauriEvent,
  invokeCalls,
  listenHandlersFor,
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
} from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

// Unique marker: only the mocked get_logs payload contains this string, so
// this assertion fails when the invoke mock is absent (empty-state renders).
const MARKER = "smoke-mock-payload-7f3a";

describe("DeveloperScreen smoke (tauri mock)", () => {
  afterEach(() => {
    cleanup();
    resetTauriMocks();
  });

  it("renders log lines from the mocked get_logs invoke", async () => {
    mockInvokeResponses({
      get_profile: {
        s1Control: { styling: "casual", structure: "prose", context: "general" },
      },
      get_settings: { selectedLlmModel: "other-model" },
      get_logs: [{ level: "Success", message: MARKER }],
    });
    mockListenCapture();

    render(<DeveloperScreen />);

    expect(await screen.findByText(MARKER)).toBeTruthy();
    expect(invokeCalls().map((c) => c.cmd)).toContain("get_logs");
    expect(listenHandlersFor("settings-changed")).toHaveLength(1);

    // Backend push path: emitting settings-changed re-invokes get_settings.
    const before = invokeCalls().filter((c) => c.cmd === "get_settings").length;
    await emitTauriEvent("settings-changed", undefined);
    await waitFor(() => {
      expect(
        invokeCalls().filter((c) => c.cmd === "get_settings").length
      ).toBeGreaterThan(before);
    });
  });
});
