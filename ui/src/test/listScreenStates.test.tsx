// claim: ui-behavior
/**
 * The app states the three list screens (AutoText, Transforms, Styles) used
 * to swallow, proven screen by screen (audit docs/app-states-audit.md):
 *
 *  - A-3: a rejected read renders LoadFailed with a retry, never the empty
 *    state, and never as "the feature is off" (A-12 for settings reads).
 *  - A-1: a rejected delete keeps the row/card and shows the failure with a
 *    Retry instead of the item vanishing.
 *  - A-2/A-6: an optimistic toggle that the backend rejects comes back to
 *    its previous position, with the failure surfaced next to the control.
 *
 * Only the states above are asserted. Everything a screen happens to render
 * while its resources are pending is deliberately out of scope.
 */
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import AutoTextScreen from "../screens/AutoTextScreen";
import TransformsScreen from "../screens/TransformsScreen";
import StylesScreen from "../screens/StylesScreen";
import { mockListenCapture, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  message: vi.fn(),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onFocusChanged: () => Promise.resolve(() => {}),
    onDragDropEvent: () => Promise.resolve(() => {}),
  }),
}));

// confirmDialog (ui/src/lib/dialogs.ts) routes to the Tauri dialog plugin
// only when it detects the Tauri runtime. jsdom doesn't set that, so force it
// once to make the delete gate reach the mocked plugin `confirm`.
beforeAll(() => {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
});

const SNIPPET = {
  id: "snip-1",
  trigger: "/me",
  snippet: "my email",
  replacement: "alex@example.com",
  description: "Personal email",
  enabled: true,
  scope: "everywhere",
  createdAt: 0,
};

const CUSTOM_STYLE = {
  id: "custom-1",
  name: "Work voice",
  description: "For Slack",
  stylePhrases: ["clear"],
  createdAt: 0,
  updatedAt: 0,
};

/**
 * Program `invoke` so every listed command resolves to its value, and every
 * command in `reject` rejects with the given message. Any other command
 * resolves to `null` (catch-all) so the screen can render without hanging on
 * a missing mock.
 */
function program(
  resolve: Record<string, unknown>,
  reject: Record<string, string> = {},
) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (cmd in reject) throw new Error(reject[cmd]);
    if (cmd in resolve) return resolve[cmd];
    return null;
  });
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
  vi.mocked(confirm).mockReset();
});

// ─────────────────────────────────────────────────────────────────────────────

describe("A-3 AutoText: a rejected list read is a retry row, not the empty state", () => {
  it("shows LoadFailed with a retry when list_autotext rejects", async () => {
    mockListenCapture();
    program(
      {
        list_system_autotext: [],
        get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
        get_usage_stats: { autotextCounts: {} },
        get_insights: { topApps: [] },
      },
      { list_autotext: "snippets file locked" },
    );
    render(<AutoTextScreen />);

    // The failed read says exactly what failed, with its own retry…
    await waitFor(() => expect(screen.getByText(/Your snippets/)).toBeTruthy());
    expect(screen.getByText(/could not be read/)).toBeTruthy();
    expect(screen.getByText(/snippets file locked/)).toBeTruthy();
    expect(screen.getByText("Try again")).toBeTruthy();
    // …and the genuinely-empty copy is never shown for a failed read.
    expect(screen.queryByText(/No snippets yet/)).toBeNull();
  });

  it("shows the empty state when list_autotext succeeds with zero rows", async () => {
    mockListenCapture();
    program({
      list_autotext: [],
      list_system_autotext: [],
      get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
      get_usage_stats: { autotextCounts: {} },
      get_insights: { topApps: [] },
    });
    render(<AutoTextScreen />);

    await waitFor(() =>
      expect(screen.getByText(/No snippets yet/)).toBeTruthy(),
    );
    // A successful empty read produces no retry row.
    expect(screen.queryByText(/could not be read/)).toBeNull();
  });

  it("treats a rejected settings read as a failure, not as the feature being off", async () => {
    mockListenCapture();
    program(
      {
        list_autotext: [SNIPPET],
        list_system_autotext: [],
        get_usage_stats: { autotextCounts: {} },
        get_insights: { topApps: [] },
      },
      { get_settings: "settings locked" },
    );
    render(<AutoTextScreen />);

    await waitFor(() =>
      expect(screen.getByText(/The snippet settings/)).toBeTruthy(),
    );
    expect(screen.getByText(/could not be read/)).toBeTruthy();
    expect(screen.getByText(/settings locked/)).toBeTruthy();
    // The toggle is not rendered at all while its source read failed.
    expect(screen.queryByRole("switch", { name: /Expand AutoText while typing/ })).toBeNull();
  });
});

describe("A-1 AutoText: a rejected delete keeps the snippet and shows the error", () => {
  it("restores the row and surfaces WriteFailed when delete_autotext rejects", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    mockListenCapture();
    program(
      {
        list_autotext: [SNIPPET],
        list_system_autotext: [],
        get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
        get_usage_stats: { autotextCounts: {} },
        get_insights: { topApps: [] },
      },
      { delete_autotext: "read-only store" },
    );
    render(<AutoTextScreen />);

    await waitFor(
      () => expect(screen.getByText("Personal email")).toBeTruthy(),
      { timeout: 3000 },
    );
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    // The write was attempted once and failed into a visible error row.
    await waitFor(
      () => expect(screen.getByText(/read-only store/)).toBeTruthy(),
      { timeout: 3000 },
    );
    expect(screen.getByText(/did not save/)).toBeTruthy();
    // And the snippet is still listed: a rejected delete is not a delete.
    // The restore comes from the re-fetch, so allow it time to settle.
    await waitFor(
      () => expect(screen.getByText("Personal email")).toBeTruthy(),
      { timeout: 3000 },
    );
  });
});

describe("A-2 AutoText: a rejected enabled-toggle reverts to its previous state", () => {
  it("flips off optimistically, then back on when update_autotext rejects", async () => {
    mockListenCapture();
    program(
      {
        list_autotext: [SNIPPET],
        list_system_autotext: [],
        get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
        get_usage_stats: { autotextCounts: {} },
        get_insights: { topApps: [] },
      },
      { update_autotext: "save failed" },
    );
    render(<AutoTextScreen />);

    const box = await screen.findByRole("checkbox", { name: /Enabled/i });
    expect((box as HTMLInputElement).checked).toBe(true);

    fireEvent.click(box);
    // The write was attempted once and rejected.
    await waitFor(() => expect(screen.getByText(/did not save/)).toBeTruthy());
    // Hold-and-restore: the checkbox is back on its saved value.
    await waitFor(() => expect((box as HTMLInputElement).checked).toBe(true));
  });
});

describe("A-6 Transforms: a rejected auto-apply toggle reverts to its previous state", () => {
  it("shows the switch once the settings read lands, and reverts it on rejection", async () => {
    mockListenCapture();
    let rejectSave = false;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "save_settings" && rejectSave) throw new Error("disk full");
      switch (cmd) {
        case "list_transforms":
          return [
            {
              id: "builtin-polish",
              name: "Polish",
              description: "Fixes grammar and tone",
              instruction: "Polish the text",
              shortcut: "",
              enabled: true,
              builtIn: true,
              autoApply: true,
            },
          ];
        case "get_settings":
          return { autoApplyTransform: true, listStyle: "bullets", polishRules: {}, polishCustomInstructions: [] };
        case "get_runtime_status":
          return {
            speech: { state: "ready", label: "Speech", modelId: "t", detail: "ready", blocking: false },
            llm: { state: "none", label: "LLM", modelId: "t", detail: "not loaded", blocking: false },
          };
        default:
          return null;
      }
    });
    render(<TransformsScreen />);

    const box = await screen.findByRole("switch", { name: /Polish every dictation automatically/ });
    expect((box as HTMLButtonElement).getAttribute("aria-checked")).toBe("true");

    rejectSave = true;
    fireEvent.click(box);
    await waitFor(() => expect(screen.getByText(/did not save/)).toBeTruthy());
    expect(screen.getByText(/disk full/)).toBeTruthy();
    // The rejected flip went back to the saved value.
    await waitFor(() =>
      expect((box as HTMLButtonElement).getAttribute("aria-checked")).toBe("true"),
    );
  });
});

describe("A-1 Styles: a rejected delete keeps the style row and shows the error", () => {
  it("restores the row and surfaces WriteFailed when delete_style_profile rejects", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    mockListenCapture();
    program(
      {
        list_style_profiles: [CUSTOM_STYLE],
        get_settings: { activeStyleProfile: "custom-1" },
        get_profile: { s1Control: { styling: "semi-formal", structure: "lists", context: "general" } },
        style_resolution: {
          profile: { styleId: "custom-1", styleName: "Work voice", winner: true, note: "custom" },
          effectivePhrases: ["clear"],
          capability: { modelId: "fast", label: "Fast", fixedBehavior: false },
        },
        get_app_style_overrides: {},
      },
      { delete_style_profile: "styles locked" },
    );
    render(<StylesScreen />);

    await waitFor(() => expect(screen.getByText("Work voice")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Delete style Work voice" }));

    await waitFor(() =>
      expect(screen.getByText(/styles locked/)).toBeTruthy(),
    );
    expect(screen.getByText(/did not save/)).toBeTruthy();
    // The row is back: a rejected delete is not a delete.
    expect(screen.getByText("Work voice")).toBeTruthy();
  });
});
