// claim: ui-behavior
/**
 * The Style tab renders from the model index, not from model ids compared in
 * the screen. The shipped default model (SpeakoFlow Mini) reads neither the
 * style phrases nor the Tone/Structure/Context controls, and the old hard-coded
 * list did not contain it, so the page offered controls that change nothing.
 *
 * Each case feeds one `style_resolution.capability` and asserts which bands
 * appear, which stay inert, and what the page says about it.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import StylesScreen from "../screens/StylesScreen";
import { mockInvokeResponses, invokeCalls, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const CONCISE = {
  id: "style-concise",
  name: "Concise",
  description: "Short, direct sentences.",
  stylePhrases: ["be concise"],
  createdAt: 0,
  updatedAt: 0,
};

type Channel = "fixedBehaviour" | "controlLine" | "stylePhrases";

const capability = (
  modelId: string,
  modelName: string,
  channel: Channel,
  stylesApply: boolean,
  readsControlLine: boolean,
  compareMeaningful: boolean
) => ({ modelId, modelName, channel, stylesApply, readsControlLine, compareMeaningful });

const SPEAKO = capability(
  "speakoflow-mini",
  "SpeakoFlow Mini",
  "fixedBehaviour",
  false,
  false,
  false
);
const S1 = capability("s1-mini", "S1-mini", "controlLine", false, true, false);
const FAST = capability("fast", "Fast (Qwen3 1.7B)", "stylePhrases", true, false, true);

/** The cascade Concise wins, with its phrases live: what a model would be told. */
const cascade = (cap: ReturnType<typeof capability>) => ({
  frontmostApp: "Gmail",
  frontmostAppType: "email",
  layers: [
    {
      rank: 4,
      key: "global",
      label: "Global pick",
      styleId: "style-concise",
      styleName: "Concise",
      winner: true,
      note: "Your global pick decides, because nothing above it applies to this app.",
    },
  ],
  effectivePhrases: ["be concise"],
  capability: cap,
});

function serve(cap: ReturnType<typeof capability>) {
  mockInvokeResponses({
    list_style_profiles: [CONCISE],
    get_settings: { activeStyleProfile: "style-concise", selectedLlmModel: cap.modelId },
    get_profile: { s1Control: { styling: "semi-formal", structure: "lists", context: "general" } },
    get_app_style_overrides: {},
    style_resolution: cascade(cap),
  });
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

/** Buttons matched on their exact label, so a disabled state is never read off
 *  an element that merely contains the word. */
const buttonNamed = (label: string): HTMLButtonElement | null =>
  (screen.getAllByRole("button") as HTMLButtonElement[]).find(
    (b) => b.textContent?.trim() === label
  ) ?? null;

describe("Style tab follows the model index", () => {
  it("explains the fixed-behaviour default model instead of offering inert controls", async () => {
    serve(SPEAKO);
    render(<StylesScreen />);

    await waitFor(() => expect(screen.getAllByText("Concise").length).toBeGreaterThan(0));

    // The reason is on the page, not only in a tooltip a keyboard user never
    // sees.
    expect(screen.getByText(/does not read these phrases/).textContent).toContain("SpeakoFlow Mini");
    expect(screen.getByText(/so both panes would come back the same/).textContent).toContain(
      "SpeakoFlow Mini writes in one fixed style"
    );
    expect(screen.getByText(/the styles below are not applied/)).toBeTruthy();
    expect(buttonNamed("Active")?.disabled).toBe(true);

    // No control band and no Compare editor: both would be inert.
    expect(screen.queryByText("Polish writing style")).toBeNull();
    expect(screen.queryByLabelText("Sentence to compare")).toBeNull();
  });

  it("shows the Tone, Structure and Context controls to a control-line model", async () => {
    serve(S1);
    render(<StylesScreen />);

    await waitFor(() => expect(screen.getByText("Polish writing style")).toBeTruthy());
    expect(screen.getByText("Tone")).toBeTruthy();
    expect(screen.getByText("Structure")).toBeTruthy();
    expect(screen.getByText("Context")).toBeTruthy();
    // S1-mini is cleanup-only, so the styles list still cannot be applied, but
    // it is the control line that replaces it, not the model's own training.
    expect(buttonNamed("Active")?.disabled).toBe(true);
    expect(screen.getByText(/^S1-mini writes Polish from the Tone/)).toBeTruthy();
    expect(screen.getByText(/does not read this list/).textContent).toContain("S1-mini");
  });

  it("lets an instruction-following model use styles and compare them", async () => {
    serve(FAST);
    render(<StylesScreen />);

    await waitFor(() => expect(screen.getAllByText("Concise").length).toBeGreaterThan(0));
    expect(buttonNamed("Active")?.disabled).toBe(false);
    expect(screen.getByLabelText("Sentence to compare")).toBeTruthy();
    expect(buttonNamed("Compare")).toBeTruthy();
    expect(screen.queryByText("Polish writing style")).toBeNull();
    expect(screen.getByText("Read by Fast (Qwen3 1.7B).")).toBeTruthy();
  });

  it("says so and offers a retry when the cascade cannot be read", async () => {
    const fallbacks: Record<string, unknown> = {
      list_style_profiles: [CONCISE],
      get_settings: { activeStyleProfile: "", selectedLlmModel: "fast" },
      get_profile: { s1Control: { styling: "semi-formal", structure: "lists", context: "general" } },
      get_app_style_overrides: {},
    };
    mockInvokeResponses(fallbacks);
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      cmd === "style_resolution"
        ? Promise.reject(new Error("platform unavailable"))
        : Promise.resolve(fallbacks[cmd])
    );
    render(<StylesScreen />);

    await waitFor(() => expect(screen.getByText("Check again")).toBeTruthy());
    // Nothing is claimed about a model we could not resolve: the list stays
    // usable and no notice invents a limitation.
    expect(buttonNamed("Activate")?.disabled).toBe(false);
    expect(screen.queryByText("Style right now")).toBeNull();
    expect(screen.queryByText(/does not read/)).toBeNull();

    const before = invokeCalls().filter((c) => c.cmd === "style_resolution").length;
    fireEvent.click(screen.getByText("Check again"));
    await waitFor(() =>
      expect(invokeCalls().filter((c) => c.cmd === "style_resolution").length).toBeGreaterThan(
        before
      )
    );
  });
});

/**
 * The shared `mockInvokeResponses` hands back the same object reference on every
 * call, so `setData(sameRef)` bails out and a reload loop is invisible. Real IPC
 * deserialises a fresh object each time, which is what let the Style tab issue
 * 30,000 `get_settings` calls in 12 seconds while four green tests watched.
 */
function serveFreshObjects(cap: ReturnType<typeof capability>) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case "list_style_profiles":
        return [{ ...CONCISE }];
      case "get_settings":
        return { activeStyleProfile: "style-concise", selectedLlmModel: cap.modelId };
      case "get_profile":
        return { s1Control: { styling: "semi-formal", structure: "lists", context: "general" } };
      case "get_app_style_overrides":
        return {};
      case "style_resolution":
        return cascade(cap);
      default:
        throw new Error(`[storm-guard] unexpected invoke: ${cmd}`);
    }
  });
}

describe("Style tab does not re-read itself", () => {
  it("issues each read a bounded number of times on mount", async () => {
    serveFreshObjects(FAST);
    render(<StylesScreen />);

    await waitFor(() => expect(screen.getAllByText("Concise").length).toBeGreaterThan(0));
    // Let any self-retriggering effect settle before counting.
    await new Promise((r) => setTimeout(r, 250));

    const counts: Record<string, number> = {};
    for (const { cmd } of invokeCalls()) counts[cmd] = (counts[cmd] ?? 0) + 1;

    // StrictMode is off in this suite, so a correct mount reads each source once.
    // Anything above 2 means a reload is re-triggering its own effect.
    for (const cmd of [
      "list_style_profiles",
      "get_settings",
      "get_profile",
      "get_app_style_overrides",
      "style_resolution",
    ]) {
      expect(
        counts[cmd] ?? 0,
        `${cmd} was called ${counts[cmd] ?? 0} times by a single mount`
      ).toBeLessThanOrEqual(2);
    }
  });
});
