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

const DECIDERS = [
  {
    id: "neudecide-q4",
    name: "NeuDecide Q4 (decider)",
    installed: true,
    available: true,
    reason: null,
  },
  {
    id: "qwen3-asr+llm",
    name: "Qwen ASR + local LLM",
    installed: false,
    available: false,
    reason: "requires a Qwen ASR speech model and a local LLM model",
  },
];

interface RenderOpts {
  hotkey?: string;
  // The decider inventory the picker offers. "absent" omits the rows entirely.
  deciders?: "installed" | "absent";
  deciderModel?: string;
}

function renderScreen(opts: RenderOpts = {}): void {
  const { hotkey = "Cmd+Shift+.", deciders = "installed", deciderModel = "neudecide-q4" } = opts;
  mockInvokeResponses({
    get_settings: {
      commandModeEnabled: true,
      commandHotkey: hotkey,
      commandDeciderModel: deciderModel,
    },
    list_command_deciders: deciders === "installed" ? DECIDERS : [],
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

  it("offers the decider models the backend reports", async () => {
    renderScreen({ deciders: "installed" });
    const select = await screen.findByRole("combobox", { name: "Decider" });
    expect(select).toBeTruthy();
    expect(screen.getByText("NeuDecide Q4 (decider)")).toBeTruthy();
    expect(
      screen.getByText("Qwen ASR + local LLM (requires a Qwen ASR speech model and a local LLM model)"),
    ).toBeTruthy();
  });

  it("selects the decider model the setting holds", async () => {
    renderScreen({ deciders: "installed", deciderModel: "qwen3-asr+llm" });
    const select = (await screen.findByRole("combobox", { name: "Decider" })) as HTMLSelectElement;
    expect(select.value).toBe("qwen3-asr+llm");
  });

  it("hides the picker when the backend reports no decider rows", async () => {
    renderScreen({ deciders: "absent" });
    expect(screen.queryByRole("combobox", { name: "Decider" })).toBeNull();
  });

  it("shows the configured hotkey on the toggle card", async () => {
    renderScreen({ hotkey: "Cmd+Shift+." });
    expect(await screen.findByText("Hotkey: Cmd+Shift+.")).toBeTruthy();
  });

  it("says the hotkey is not set when the setting is empty", async () => {
    renderScreen({ hotkey: "" });
    expect(await screen.findByText("Hotkey: not set (wake phrase still works)")).toBeTruthy();
  });
});
