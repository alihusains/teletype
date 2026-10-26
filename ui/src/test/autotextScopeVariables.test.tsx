// claim: ui-behavior
import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import AutoTextScreen from "../screens/AutoTextScreen";
import { mockInvokeResponses, mockListenCapture, invokeCalls, resetTauriMocks } from "./tauriMock";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
  open: vi.fn(),
  confirm: vi.fn(),
  message: vi.fn(),
}));

const ENTRY = {
  id: "e1",
  trigger: "/email",
  replacement: "alex@example.com",
  description: "Personal email",
  enabled: true,
  scope: "everywhere",
  createdAt: 1,
  updatedAt: 1,
  snippet: "my email",
};

function seed(extra: Record<string, unknown> = {}) {
  mockInvokeResponses({
    list_autotext: [ENTRY],
    list_system_autotext: [],
    get_usage_stats: { autotextCounts: {} },
    get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
    get_insights: { topApps: [] },
    ...extra,
  });
}

function openCreate() {
  render(<AutoTextScreen />);
  fireEvent.click(screen.getByRole("button", { name: "+ Add new" }));
  return {
    trigger: screen.getByPlaceholderText("/email"),
    replacement: screen.getByPlaceholderText("abcd@gmail.com"),
  };
}

describe("AutoText scope and variables", () => {
  beforeEach(() => {
    resetTauriMocks();
    vi.mocked(listen).mockResolvedValue(() => {});
    mockListenCapture();
    seed();
  });

  it("inserts a variable at the caret instead of appending blindly", () => {
    const { replacement } = openCreate();
    fireEvent.change(replacement, { target: { value: "Sent from " } });
    (replacement as HTMLTextAreaElement).setSelectionRange(5, 5);

    fireEvent.click(screen.getByRole("button", { name: "{{app}}" }));

    expect((replacement as HTMLTextAreaElement).value).toBe("Sent {{app}}from ");
  });

  it("offers every supported variable with a plain-language meaning", () => {
    openCreate();
    for (const token of ["{{date}}", "{{time}}", "{{clipboard}}", "{{app}}"]) {
      expect(screen.getByRole("button", { name: token })).toBeTruthy();
    }
    expect(screen.getByText(/frontmost app/i)).toBeTruthy();
  });

  it("saves a one-app scope on create", async () => {
    const { trigger, replacement } = openCreate();
    fireEvent.change(trigger, { target: { value: "/sig" } });
    fireEvent.change(replacement, { target: { value: "Sent from {{app}}" } });
    fireEvent.click(screen.getByRole("radio", { name: "One app" }));
    fireEvent.change(screen.getByPlaceholderText("Slack or com.tinyspeck.slackmacgap"), {
      target: { value: "com.google.gmail" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add snippet" }));

    await waitFor(() =>
      expect(invokeCalls()).toContainEqual({
        cmd: "create_autotext",
        args: {
          entry: expect.objectContaining({
            trigger: "/sig",
            replacement: "Sent from {{app}}",
            scope: "com.google.gmail",
          }),
        },
      }),
    );
  });

  it("refuses a one-app scope with no app, without calling the backend", async () => {
    const { trigger, replacement } = openCreate();
    fireEvent.change(trigger, { target: { value: "/sig" } });
    fireEvent.change(replacement, { target: { value: "Sent from {{app}}" } });
    fireEvent.click(screen.getByRole("radio", { name: "One app" }));
    fireEvent.click(screen.getByRole("button", { name: "Add snippet" }));

    expect((await screen.findByRole("alert")).textContent).toContain(
      "Choose an app, or switch to Everywhere.",
    );
    expect(invokeCalls().some((c) => c.cmd === "create_autotext")).toBe(false);
  });

  it("preselects the stored scope when editing", async () => {
    seed({
      list_autotext: [{ ...ENTRY, scope: "com.google.gmail" }],
    });
    render(<AutoTextScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "Edit" }));

    const oneApp = screen.getByRole("radio", { name: "One app" });
    expect(oneApp.getAttribute("aria-checked")).toBe("true");
    expect(
      (screen.getByPlaceholderText("Slack or com.tinyspeck.slackmacgap") as HTMLInputElement).value,
    ).toBe("com.google.gmail");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(invokeCalls()).toContainEqual({
        cmd: "update_autotext",
        args: { entry: expect.objectContaining({ scope: "com.google.gmail" }) },
      }),
    );
  });

  it("shows which app an entry is limited to", async () => {
    seed({ list_autotext: [{ ...ENTRY, scope: "Slack" }] });
    render(<AutoTextScreen />);
    expect(await screen.findByText("Only in Slack")).toBeTruthy();
  });

  it("suggests apps the user actually dictates into", async () => {
    seed({
      get_insights: {
        topApps: [
          { label: "Slack — #platform-eng-and-deployments", count: 12, share: 1 },
          { label: "Gmail", count: 5, share: 0.4 },
          { label: "gmail", count: 2, share: 0.2 },
          { label: "File: board-readout.wav", count: 1, share: 0.1 },
        ],
      },
    });
    openCreate();
    fireEvent.click(screen.getByRole("radio", { name: "One app" }));
    fireEvent.click(await screen.findByRole("button", { name: "Use Slack" }));

    expect(
      (screen.getByPlaceholderText("Slack or com.tinyspeck.slackmacgap") as HTMLInputElement).value,
    ).toBe("Slack");
    // Window titles are stripped and duplicates collapse; file names are not apps.
    expect(screen.getAllByRole("button", { name: "Use Gmail" })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: /Use File:/ })).toBeNull();
  });
});
