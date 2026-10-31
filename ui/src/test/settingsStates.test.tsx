// claim: ui-behavior
/**
 * A/B/C/D-1..5 — the app states the Settings, Keybinds, and Dictionary
 * screens used to swallow: a save_settings that rejects, a failed
 * list_user_packs read, a keybind save racing a second one, and the
 * learn-from-edits toggle that used to write a field the engine never reads.
 *
 * Each test proves the *state* the user should see, not just that a command
 * fired: the restored previous value, the LoadFailed vs empty distinction,
 * the double-submit guard, and the real personalization gate.
 */
import { afterEach, describe, expect, it, vi, beforeAll } from "vitest";
import { cleanup, render, screen, waitFor, fireEvent } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import SettingsScreen from "../screens/SettingsScreen";
import KeybindsScreen from "../screens/KeybindsScreen";
import DictionaryScreen from "../screens/DictionaryScreen";
import {
  mockInvokeResponses,
  mockListenCapture,
  resetTauriMocks,
  invokeCalls,
} from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  message: vi.fn(),
}));
// SettingsScreen pulls its rainbow visuals from the pill module; mock them out
// so these behavior tests stay focused.
vi.mock("../pill", async () => {
  const noop = () => null;
  return {
    default: noop,
    RainbowLips: noop,
    RainbowMeter: noop,
    rainbowColor: (t: number) => `hsl(${Math.round(t * 360)}, 80%, 50%)`,
  };
});
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onFocusChanged: () => Promise.resolve(() => {}),
    onDragDropEvent: () => Promise.resolve(() => {}),
  }),
}));

// A controllable recorder. Both recorder components take (value, onSave); we
// replace them with a plain button that, on click, fires onSave with a fixed
// hotkey. We partial-mock via importOriginal so every other export
// (displayHotkey, usesWebviewCapture) keeps working for the screen and the
// clipboard section that also render recorders.
let commitValue = "Cmd+Shift+A";
vi.mock("../components/HotkeyRecorder", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../components/HotkeyRecorder")>();
  return {
    ...actual,
    default: (props: { value: string; onSave: (v: string) => void }) => (
      <button aria-label="test-recorder" onClick={() => props.onSave(commitValue)}>
        {props.value}
      </button>
    ),
  };
});
vi.mock("../components/WebviewHotkeyRecorder", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../components/WebviewHotkeyRecorder")>();
  return {
    ...actual,
    default: (props: { value: string; onSave: (v: string) => void }) => (
      <button aria-label="test-webview-recorder" onClick={() => props.onSave(commitValue)}>
        {props.value}
      </button>
    ),
  };
});

// confirmDialog (ui/src/lib/dialogs.ts) routes to the Tauri dialog plugin only
// when it detects the Tauri runtime. jsdom doesn't set that, so force it once
// to make the delete-packs gate reach the mocked plugin `confirm`.
beforeAll(() => {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
});

// The full valid settings object the screens read on mount. Mirrors the Rust
// Settings struct (camelCase); a command without a response here rejects, so
// this must stay complete.
const SETTINGS: Record<string, unknown> = {
  hotkey: "Cmd+Shift+Space",
  recordingMode: "auto",
  selectedSpeechModel: "test-model",
  language: "en-US",
  inputDevice: "default",
  restoreClipboard: true,
  keepTextOnClipboard: false,
  tapToPasteClipboard: false,
  autoApplyTransform: false,
  showTrayIcon: true,
  hasCompletedOnboarding: true,
  selectedLlmModel: "other-model",
  typingAutotextEnabled: false,
  autotextTiming: "immediate",
  removeFillerWords: true,
  fillerWords: [],
  pillPosition: "topRight",
  alwaysShowPill: false,
  livePreviewEnabled: false,
  cancelHotkey: "Escape",
  smartInsertion: true,
  playRecordingSounds: false,
  recordingSound: "ding",
  warmEnginePolicy: "keep",
  matchStrictness: "standard",
  dictionaryEnabled: true,
  pillStyle: "default",
  appIcon: "white",
  transcriptsDir: "",
  vadAutoStop: true,
  vadSilenceMs: 700,
  noiseReductionEnabled: false,
  enableDeveloperTab: false,
  polishGateEnabled: false,
  polishGateThresholdWords: 10,
  polishMaxChangePercent: 40,
  restoreEmoji: false,
  spokenEmoji: false,
  spokenPunctuation: false,
  modelUnloadDelaySecs: 60,
  activeStyleProfile: "default",
  appLanguageOverrides: {},
  appStyleOverrides: {},
  enabledPacks: [],
  selectedLlmProvider: "builtin",
  theme: "system",
  reduceMotion: false,
  polishRules: {},
  listStyle: "bullets",
  quickAddHotkey: "Cmd+Shift+K",
  commandHotkey: "Cmd+Shift+.",
  commandWakePhrase: false,
  pendingNotice: null,
  polishWholeTextAtEnd: false,
  polishSkipWhenClean: true,
  instantStart: "always",
  preferBuiltinMicWithBt: false,
  audioConditioning: false,
  learnFromEdits: true,
  learnNamesFromScreen: true,
  keepAudioForSelfTests: false,
};

const RUNTIME_STATUS = {
  speech: { state: "ready", label: "Speech", modelId: "test", detail: "ready", blocking: false },
  llm: { state: "none", label: "LLM", modelId: "test", detail: "not loaded", blocking: false },
};

// A save_settings that resolves only when the test releases it — used to keep a
// write "in flight" while a second one is attempted.
function controllableSave() {
  let resolve!: (v: unknown) => void;
  const pending = new Promise((r) => {
    resolve = r;
  });
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (cmd === "save_settings") return pending;
    throw new Error(`[tauriMock] unexpected invoke: ${cmd}`);
  });
  return { release: () => resolve(null) };
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
  vi.mocked(confirm).mockReset();
});

describe("A-1 Settings: a failed save restores the value and shows WriteFailed", () => {
  it("rolls back the pill position and surfaces a write error when save_settings rejects", async () => {
    // save_settings must reject (not just return a function value), so drive a
    // direct invoke mock implementation for it while the reads stay static.
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "save_settings") throw new Error("disk full");
      switch (cmd) {
        case "get_settings":
          return SETTINGS;
        case "list_input_devices":
          return [];
        case "list_style_profiles":
          return [];
        case "get_app_language_overrides":
          return {};
        case "get_app_style_overrides":
          return {};
        case "get_transcripts_dir":
          return "";
        case "get_runtime_status":
          return RUNTIME_STATUS;
        default:
          throw new Error(`[tauriMock] unexpected invoke: ${cmd}`);
      }
    });
    mockListenCapture();
    render(<SettingsScreen active={false} />);
    await waitFor(() => expect(screen.queryByText("Loading…")).toBeNull());

    const select = screen.getByRole("combobox", { name: /pill position/i }) as HTMLSelectElement;
    expect(select.value).toBe("topRight");

    await fireEvent.change(select, { target: { value: "bottomLeft" } });

    // The rejected write surfaces an error alert and the control reverts to the
    // previous value.
    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeTruthy();
    });
    expect((screen.getByRole("combobox", { name: /pill position/i }) as HTMLSelectElement).value)
      .toBe("topRight");

    const saves = invokeCalls().filter((c) => c.cmd === "save_settings");
    expect(saves).toHaveLength(1);
  });
});

describe("A-2 Settings: learn-from-edits drives the real personalization gate", () => {
  it("calls set_personalization_enabled when the toggle flips", async () => {
    mockInvokeResponses({
      get_settings: SETTINGS,
      get_profile: {
        language: "en-US",
        preferences: [],
        learnFromEdits: false,
        learnAppSpecific: false,
        learnTerminology: false,
      },
      set_personalization_enabled: null,
      list_input_devices: [],
      list_style_profiles: [],
      get_app_language_overrides: {},
      get_app_style_overrides: {},
      get_transcripts_dir: "",
      get_runtime_status: RUNTIME_STATUS,
    });
    mockListenCapture();
    render(<SettingsScreen active={false} />);
    await waitFor(() => expect(screen.queryByText("Loading…")).toBeNull());
    await waitFor(() => {
      expect(invokeCalls().some((c) => c.cmd === "get_profile")).toBe(true);
    });

    // The profile says learnFromEdits=false, so the toggle starts off; flip it on.
    const toggle = screen.getByRole("switch", { name: "Learn from my edits" });
    fireEvent.click(toggle);

    await waitFor(() => {
      const gateCalls = invokeCalls().filter((c) => c.cmd === "set_personalization_enabled");
      expect(gateCalls).toHaveLength(1);
      expect((gateCalls[0].args as { enabled: boolean }).enabled).toBe(true);
    });
  });
});

describe("A-3 Keybinds: the recorder cannot double-submit an in-flight save", () => {
  it("ignores a second hotkey commit while the first save is still in flight", async () => {
    mockInvokeResponses({
      get_settings: SETTINGS,
      list_transforms: [],
    });
    mockListenCapture();

    const { unmount } = render(<KeybindsScreen />);
    await waitFor(() => {
      expect(screen.getByText("Currently registered")).toBeTruthy();
    });

    const saver = controllableSave();

    // The record-hotkey row is the first recorder rendered; all mock recorders
    // share the webview label, so take the first one.
    const recorders = screen.getAllByRole("button", { name: "test-webview-recorder" });
    expect(recorders.length).toBeGreaterThan(0);
    const recorder = recorders[0];
    expect(recorder).toBeTruthy();

    // First commit: the save starts and stays pending.
    fireEvent.click(recorder);
    await waitFor(() => {
      expect(invokeCalls().filter((c) => c.cmd === "save_settings")).toHaveLength(1);
    });

    // Second commit while the first is still in flight: the per-field guard
    // (writes.busy("hotkey")) must drop it.
    fireEvent.click(screen.getAllByRole("button", { name: "test-webview-recorder" })[0]);

    // Let a couple of microtask turns elapse so the (guarded) second call can't
    // sneak in, then release the first save.
    await new Promise((r) => setTimeout(r, 0));
    saver.release();
    await waitFor(() => {
      expect(invokeCalls().filter((c) => c.cmd === "save_settings")).toHaveLength(1);
    });

    unmount();
    expect(invokeCalls().filter((c) => c.cmd === "save_settings")).toHaveLength(1);
  });
});

describe("A-3b Keybinds: a rejected save restores the binding on its own row", () => {
  it("puts the old hotkey back and shows the row error when save_settings rejects", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "save_settings") throw new Error("read-only volume");
      if (cmd === "get_settings") return SETTINGS;
      if (cmd === "list_transforms") return [];
      throw new Error(`[tauriMock] unexpected invoke: ${cmd}`);
    });
    mockListenCapture();
    render(<KeybindsScreen />);
    await waitFor(() => {
      expect(screen.getByText("Currently registered")).toBeTruthy();
    });

    const recorder = () => screen.getAllByRole("button", { name: "test-webview-recorder" })[0];
    expect(recorder().textContent).toBe("Cmd+Shift+Space");

    fireEvent.click(recorder());

    // The row reverts to what the backend actually holds, and the reason lands
    // on that row rather than at the foot of a long scroll.
    await waitFor(() => {
      expect(recorder().textContent).toBe("Cmd+Shift+Space");
    });
    const alerts = screen.getAllByRole("alert");
    expect(alerts.some((a) => a.textContent?.includes("read-only volume"))).toBe(true);
  });

  it("makes the recorder inert while its own save is in flight", async () => {
    mockInvokeResponses({ get_settings: SETTINGS, list_transforms: [] });
    mockListenCapture();
    render(<KeybindsScreen />);
    await waitFor(() => {
      expect(screen.getByText("Currently registered")).toBeTruthy();
    });

    const saver = controllableSave();
    const row = () => screen.getAllByRole("button", { name: "test-webview-recorder" })[0];
    fireEvent.click(row());
    await waitFor(() => {
      expect(screen.getByText("Saving…")).toBeTruthy();
    });
    // pointer-events:none on the recorder wrapper is what makes the second
    // keystroke physically unreachable, not just ignored.
    const wrapper = row().parentElement as HTMLElement;
    expect(wrapper.style.pointerEvents).toBe("none");
    saver.release();
  });
});

describe("D-3/D-5 Dictionary: a failed pack read is LoadFailed, not empty", () => {  // mockInvokeResponses only stores static values, so a failing command must
  // return a rejected promise via a direct invoke mock implementation.
  function mockDictInvoke(overrides: Record<string, unknown>): void {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      const base: Record<string, unknown> = {
        get_settings: SETTINGS,
        list_dictionary: [],
        list_packs: [],
        list_user_packs: [],
      };
      const val = (overrides as Record<string, unknown>)[cmd] ?? base[cmd];
      if (typeof val === "function") return (val as () => unknown)();
      if (val instanceof Error) throw val;
      return val;
    });
  }

  it("shows a retryable error when list_user_packs rejects", async () => {
    mockDictInvoke({ list_user_packs: () => Promise.reject(new Error("no such file")) });
    mockListenCapture();
    render(<DictionaryScreen />);

    fireEvent.click(screen.getByRole("button", { name: /vocabulary packs/i }));

    await waitFor(() => {
      // LoadFailed renders with role="alert" and the raw backend reason.
      expect(screen.getByText("no such file")).toBeTruthy();
    });
    // The pack header (the empty-list affordance) is not the state we're in.
    expect(screen.queryByText(/domain term packs, plus your own/i)).toBeNull();
  });

  it("distinguishes a genuinely empty pack list from a failed read", async () => {
    mockDictInvoke({ list_user_packs: () => Promise.resolve([]) });
    mockListenCapture();
    render(<DictionaryScreen />);
    fireEvent.click(screen.getByRole("button", { name: /vocabulary packs/i }));

    await waitFor(() => {
      expect(screen.getByText(/domain term packs, plus your own/i)).toBeTruthy();
    });
    // A successful empty read must not surface the failed-read message.
    expect(screen.queryByText("no such file")).toBeNull();
  });
});

describe("D-4 Dictionary: a failed delete_user_pack leaves the pack in the list", () => {
  it("does not remove the pack and shows an error when delete rejects", async () => {
    const userPack = {
      id: "userpack-1",
      name: "My Company",
      description: "internal terms",
      termCount: 3,
      enabled: true,
    };
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case "get_settings":
          return SETTINGS;
        case "list_dictionary":
          return [];
        case "list_packs":
          return [];
        case "list_user_packs":
          return [userPack];
        case "delete_user_pack":
          throw new Error("read-only");
        default:
          throw new Error(`[tauriMock] unexpected invoke: ${cmd}`);
      }
    });
    mockListenCapture();
    vi.mocked(confirm).mockResolvedValue(true);

    render(<DictionaryScreen />);
    fireEvent.click(screen.getByRole("button", { name: /vocabulary packs/i }));

    await waitFor(() => {
      expect(screen.getByText("My Company")).toBeTruthy();
    });

    // Open the pack, then hit its delete (trash) button.
    fireEvent.click(screen.getByText("My Company"));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /delete pack/i })).toBeTruthy();
    });
    screen.getByRole("button", { name: /delete pack/i }).click();

    // The confirm gate fired and the write was attempted exactly once.
    await waitFor(() => {
      expect(vi.mocked(confirm)).toHaveBeenCalled();
      expect(invokeCalls().filter((c) => c.cmd === "delete_user_pack")).toHaveLength(1);
    });

    // The failed write surfaces an error, and the pack is still listed (refresh
    // only happens on success).
    await waitFor(() => {
      expect(screen.queryAllByRole("alert").length).toBeGreaterThan(0);
    });
    expect(screen.getByText("My Company")).toBeTruthy();
  });
});
