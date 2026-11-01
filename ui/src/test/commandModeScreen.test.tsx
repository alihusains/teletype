// claim: ui-behavior
/**
 * The Command Mode screen is the user-facing contract for the command
 * pipeline: the tool table must match the 14 tools the executor can run,
 * the decider line must reflect what is actually on disk (no claim the
 * app cannot back), and the hotkey line must show the real binding or
 * say it is not set.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import CommandModeScreen from "../screens/CommandModeScreen";
import { mockInvokeResponses, mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

// The 14 tools the executor runs (crates/teletype-desktop/src/command_executor.rs).
const TOOL_NAMES = [
  "open_app",
  "open_url",
  "open_file",
  "open_settings",
  "keyboard_shortcut",
  "type_text",
  "compose_email",
  "send_message",
  "create_calendar_event",
  "set_timer",
  "set_volume",
  "mute_speaker",
  "set_brightness",
  "play_media",
];

const MODEL = {
  id: "neudecide-q4",
  name: "NeuDecide Q4 (decider)",
  sizeMb: 41,
  downloaded: true,
};

interface RenderOpts {
  hotkey?: string;
  // Omit the decider row entirely, or present it with downloaded: false.
  models?: "installed" | "not-installed" | "absent";
}

function renderScreen(opts: RenderOpts = {}): void {
  const { hotkey = "Cmd+Shift+.", models = "installed" } = opts;
  const listModels =
    models === "installed"
      ? [MODEL]
      : models === "not-installed"
        ? [{ ...MODEL, downloaded: false }]
        : [];
  mockInvokeResponses({
    get_settings: {
      commandModeEnabled: true,
      commandHotkey: hotkey,
    },
    list_models: listModels,
    save_settings: null,
  });
  mockListenCapture();
  render(<CommandModeScreen />);
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

describe("Command Mode screen", () => {
  it("lists all 14 executor tools", async () => {
    renderScreen();
    for (const tool of TOOL_NAMES) {
      expect(await screen.findByText(tool)).toBeTruthy();
    }
  });

  it("labels send_message as the tool that acts without a draft step", async () => {
    renderScreen();
    expect(await screen.findByText("Sends immediately")).toBeTruthy();
  });

  it("shows the installed decider when neudecide-q4 is downloaded", async () => {
    renderScreen({ models: "installed" });
    expect(await screen.findByText("Decider: NeuDecide Q4 installed (41 MB)")).toBeTruthy();
  });

  it("shows the not-installed decider when the model is missing", async () => {
    renderScreen({ models: "absent" });
    expect(await screen.findByText("Decider: not installed")).toBeTruthy();
  });

  it("shows the not-installed decider when the model row says not downloaded", async () => {
    renderScreen({ models: "not-installed" });
    expect(await screen.findByText("Decider: not installed")).toBeTruthy();
  });

  it("shows the configured hotkey on the toggle card", async () => {
    renderScreen({ hotkey: "Cmd+Shift+." });
    expect(await screen.findByText("Hotkey: Cmd+Shift+.")).toBeTruthy();
  });

  it("says the hotkey is not set when the setting is empty", async () => {
    renderScreen({ hotkey: "" });
    expect(await screen.findByText("Hotkey: not set")).toBeTruthy();
  });
});
