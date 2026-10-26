// claim: ui-behavior
/**
 * The Personalization tab has to answer "what did it learn, from what, and is
 * it actually doing anything?" The old screen showed the rule text and a
 * confidence word invented in the UI ("new"/"building"/"strong") that matched
 * nothing in the Rust gate, and it buried the master switch's real consequence.
 *
 * These cases pin the tab to the four things that are code-true:
 *   - explicit rules apply from the first dictation, learned rules need two
 *     observations (`packet::resolve`: `explicit || count >= 2`),
 *   - turning "Learn from my edits" off suppresses every preference,
 *   - a learned word correction carries a (learnedFrom, learnedTo) pair,
 *   - the wire is camelCase (`createdAt`/`updatedAt`, BUG-07).
 *
 * The profile mock is stateful on purpose. Every write here is optimistic and
 * then reloads, so a stateless mock would show the optimistic row and never
 * prove the round trip; a stateful one fails if the screen sends a payload the
 * backend would not store.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import PersonalizationScreen from "../screens/PersonalizationScreen";
import { invokeCalls, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const DAY = 86_400_000;
const NOW = 1_760_000_000_000;

type Pref = Record<string, unknown>;

const pref = (over: Pref): Pref => ({
  id: "p",
  description: "a rule",
  phrase: "a rule",
  explicit: false,
  scope: "global",
  count: 0,
  createdAt: NOW - 2 * DAY,
  updatedAt: NOW - 2 * DAY,
  ...over,
});

// The four shapes the tab distinguishes: a rule the user wrote, a word
// correction learned with its heard/written pair, an app-scoped rule, and one
// single observation that is not in force yet.
const MINE = pref({ id: "mine", description: "Always write Q3", explicit: true });
const TERM = pref({
  id: "term",
  description: "Use 'Kubernetes', not 'kubernetes'",
  count: 6,
  learnedFrom: "kubernetes",
  learnedTo: "Kubernetes",
});
const APPRULE = pref({
  id: "apprule",
  description: "Sign off as Ali in email",
  scope: { appType: "email" },
  count: 3,
});
const WAITING = pref({
  id: "waiting",
  description: "Use 'gRPC' not 'G-R-P-C'",
  scope: { appType: "chat" },
  count: 1,
});

/** A stand-in for the stored profile that the write commands actually mutate. */
function statefulProfile(
  preferences: Pref[] = [MINE, TERM, APPRULE, WAITING],
  gates = { learnFromEdits: true, learnAppSpecific: true, learnTerminology: true }
) {
  const store = { preferences: [...preferences], ...gates };
  const handlers: Record<string, (args: never) => unknown> = {
    get_profile: () => ({ language: "en-US", ...store }),
    get_app_language_overrides: () => ({}),
    add_preference: (a: { preference: Pref }) => {
      store.preferences = [a.preference, ...store.preferences];
      return a.preference;
    },
    update_preference: (a: { id: string; description: string; phrase: string }) => {
      store.preferences = store.preferences.map((x) =>
        x.id === a.id ? { ...x, description: a.description, phrase: a.phrase } : x
      );
      return null;
    },
    remove_preference: (a: { id: string }) => {
      store.preferences = store.preferences.filter((x) => x.id !== a.id);
      return null;
    },
    clear_learned: () => {
      store.preferences = store.preferences.filter((x) => x.explicit);
      return null;
    },
    // set_personalization_enabled flips all three gates together (T5.9), so the
    // test can assert the master row and the sub-gate rows agree afterwards.
    set_personalization_enabled: (a: { enabled: boolean }) => {
      store.learnFromEdits = a.enabled;
      store.learnAppSpecific = a.enabled;
      store.learnTerminology = a.enabled;
      return null;
    },
    set_profile_settings: (a: Record<string, boolean>) => {
      Object.assign(store, a);
      return null;
    },
    set_app_language_override: () => null,
    list_speech_languages: () => [{ code: "en", name: "English" }],
  };
  vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
    const h = handlers[cmd];
    if (!h) return Promise.reject(new Error(`[personalization test] unexpected invoke: ${cmd}`));
    return Promise.resolve(h(args as never));
  });
  return store;
}

afterEach(() => {
  cleanup();
  resetTauriMocks();
});

describe("Personalization states where each rule came from", () => {
  it("separates a rule I added from one it learned, and says which is in force", async () => {
    statefulProfile();
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText("Always write Q3")).toBeTruthy());

    // Mine: explicit, so in force from the first dictation, and the proof is
    // that I wrote it.
    expect(screen.getByText("You added this")).toBeTruthy();
    // Three of the four clear the gate (`explicit || count >= 2`).
    expect(screen.getAllByText("In force")).toHaveLength(3);
    expect(screen.getByText("Needs one more match")).toBeTruthy();
    expect(screen.getByText("Learned from 6 edits")).toBeTruthy();
    expect(screen.getByText("Learned from 1 edit")).toBeTruthy();
  });

  it("shows the heard/written pair a learned word correction is built from", async () => {
    statefulProfile();
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText("Always write Q3")).toBeTruthy());
    expect(screen.getByText(/Hears .kubernetes., writes .Kubernetes./)).toBeTruthy();
  });

  it("counts the summary strip against the real gate, not against the list length", async () => {
    statefulProfile();
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText("Always write Q3")).toBeTruthy());
    expect(screen.getByText("3 in force")).toBeTruthy();
    expect(screen.getByText("3 learned from your edits")).toBeTruthy();
    expect(screen.getByText("1 you added")).toBeTruthy();
    expect(screen.getByText("1 waiting for a second match")).toBeTruthy();
  });

  it("filters by scope and groups app-scoped rules under the app they belong to", async () => {
    statefulProfile();
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText("Always write Q3")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Everywhere (2)" }));
    expect(screen.getByText("Always write Q3")).toBeTruthy();
    expect(screen.getByText("Use 'Kubernetes', not 'kubernetes'")).toBeTruthy();
    expect(screen.queryByText("Sign off as Ali in email")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "In specific apps (2)" }));
    // The group heading, not the scope chip on the row.
    expect(screen.getByRole("heading", { name: "Email apps" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Chat apps" })).toBeTruthy();
    expect(screen.getByText("Sign off as Ali in email")).toBeTruthy();
    expect(screen.queryByText("Use 'Kubernetes', not 'kubernetes'")).toBeNull();
  });

  it("teaches the next step when a scope bucket is empty", async () => {
    statefulProfile([MINE]);
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText("Always write Q3")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "In specific apps (0)" }));
    expect(screen.getByText(/Nothing learned for a specific app yet/)).toBeTruthy();
  });

  it("teaches the next step when nothing is known at all", async () => {
    statefulProfile([]);
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText(/Two ways to start/)).toBeTruthy());
  });
});

describe("Personalization master switch tells the truth", () => {
  const OFF = { learnFromEdits: false, learnAppSpecific: false, learnTerminology: false };

  it("says the rules stop being applied and leaves only two sub-gates", async () => {
    statefulProfile([], OFF);
    render(<PersonalizationScreen />);
    await waitFor(() =>
      expect(screen.getByText(/none of the rules below/)).toBeTruthy()
    );

    const master = screen.getByRole("checkbox", {
      name: /Learn from my edits/,
    }) as HTMLInputElement;
    expect(master.checked).toBe(false);
    // Only the two real sub-gates remain: the old screen listed `learnFromEdits`
    // twice, once as the master and once as a row that could never disagree.
    const subGates = screen.getAllByRole("checkbox").filter((c) => c !== master);
    expect(subGates).toHaveLength(2);
    const labels = subGates.map((c) => c.closest("label")?.textContent ?? "");
    expect(labels.join(" ")).toContain("Words I keep correcting");
    expect(labels.join(" ")).toContain("Rules for one kind of app");
    // Off does not merely grey them out; the gate is unreachable so a click
    // cannot silently re-arm a sub-gate under a disabled master.
    for (const c of subGates) {
      let el: HTMLElement | null = c as HTMLElement;
      let locked = false;
      while (el) {
        if (el.style.pointerEvents === "none") locked = true;
        el = el.parentElement;
      }
      expect(locked).toBe(true);
    }
  });

  it("turns learning back on through the one command that flips all three gates", async () => {
    const store = statefulProfile([], OFF);
    render(<PersonalizationScreen />);
    const master = await screen.findByRole("checkbox", { name: /Learn from my edits/ });

    fireEvent.click(master);
    await waitFor(() =>
      expect(
        invokeCalls().some(
          (c) =>
            c.cmd === "set_personalization_enabled" &&
            (c.args as { enabled: boolean }).enabled === true
        )
      ).toBe(true)
    );
    await waitFor(() => expect(master).toHaveProperty("checked", true));
    expect(store.learnAppSpecific).toBe(true);
    expect(store.learnTerminology).toBe(true);
  });

  it("writes a new rule with camelCase timestamps so serde does not drop them", async () => {
    statefulProfile([]);
    render(<PersonalizationScreen />);
    await waitFor(() =>
      expect(screen.getByLabelText("A rule Teletype should always follow")).toBeTruthy()
    );

    fireEvent.change(screen.getByLabelText("A rule Teletype should always follow"), {
      target: { value: "Always write Q3" },
    });
    fireEvent.change(screen.getByLabelText("Applies to"), { target: { value: "email" } });
    fireEvent.click(screen.getByRole("button", { name: "Add rule" }));

    await waitFor(() => {
      const call = invokeCalls().find((c) => c.cmd === "add_preference");
      expect(call).toBeTruthy();
    });
    const arg = (invokeCalls().find((c) => c.cmd === "add_preference") as {
      args: { preference: Record<string, unknown> };
    }).args.preference;
    expect(arg.description).toBe("Always write Q3");
    expect(arg.scope).toEqual({ appType: "email" });
    expect(arg).toHaveProperty("createdAt");
    expect(arg).not.toHaveProperty("created_at");
    // It survives the reload, and an explicit rule is in force immediately.
    await waitFor(() => expect(screen.getByText("Always write Q3")).toBeTruthy());
    expect(screen.getByText("You added this")).toBeTruthy();
  });

  it("drops a rule from the list and it stays dropped", async () => {
    const store = statefulProfile();
    render(<PersonalizationScreen />);
    await waitFor(() => expect(screen.getByText("Always write Q3")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: /Forget .Always write Q3/ }));
    await waitFor(() => expect(screen.queryByText("Always write Q3")).toBeNull());
    expect(store.preferences.map((p) => p.id)).not.toContain("mine");
  });

  it("edits a learned rule through the real update, not only local state", async () => {
    statefulProfile([TERM]);
    render(<PersonalizationScreen />);
    await waitFor(() =>
      expect(screen.getByText("Use 'Kubernetes', not 'kubernetes'")).toBeTruthy()
    );

    fireEvent.click(screen.getByRole("button", { name: /Rewrite .Use/ }));
    fireEvent.change(screen.getByLabelText(/Rewrite the rule/), {
      target: { value: "Always capitalise Kubernetes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(
        invokeCalls().some(
          (c) =>
            c.cmd === "update_preference" &&
            (c.args as Record<string, string>).description === "Always capitalise Kubernetes"
        )
      ).toBe(true)
    );
    await waitFor(() => expect(screen.getByText("Always capitalise Kubernetes")).toBeTruthy());
  });

  it("rolls a failed per-app language override back instead of leaving it displayed", async () => {
    const stored: { lang: string } = { lang: "en" };
    statefulProfile([]);
    // Keep the profile handler for everything except the three language
    // commands, which get the failing-write behaviour under test.
    const base = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "get_app_language_overrides") return Promise.resolve({ notion: stored.lang });
      if (cmd === "list_speech_languages")
        return Promise.resolve([
          { code: "en", name: "English" },
          { code: "de", name: "German" },
        ]);
      if (cmd === "set_app_language_override") {
        // The write always fails here, and it never touches what is stored.
        return Promise.reject(new Error("settings file is locked"));
      }
      return base(cmd, args as Record<string, unknown> | undefined) as Promise<unknown>;
    });
    render(<PersonalizationScreen />);
    await waitFor(() =>
      expect(screen.getByLabelText("Speech language for notion")).toBeTruthy()
    );

    fireEvent.change(screen.getByLabelText("Speech language for notion"), {
      target: { value: "de" },
    });

    await waitFor(() =>
      expect(screen.getByText(/did not save: settings file is locked/)).toBeTruthy()
    );
    // The row shows the language that is actually stored, not the one clicked.
    expect(stored.lang).toBe("en");
    expect((screen.getByLabelText("Speech language for notion") as HTMLSelectElement).value).toBe(
      "en"
    );
  });

  it("fails loudly and offers a retry instead of an empty tab", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      cmd === "get_profile" ? Promise.reject(new Error("locked")) : Promise.resolve(null)
    );
    render(<PersonalizationScreen />);

    await waitFor(() =>
      expect(screen.getByText(/could not read your rules/)).toBeTruthy()
    );
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
    expect(screen.queryByText("What it knows")).toBeNull();
  });
});
