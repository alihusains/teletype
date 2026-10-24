// claim: ui-behavior
import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save, open } from "@tauri-apps/plugin-dialog";
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

function seed() {
  mockInvokeResponses({
    list_autotext: [ENTRY],
    list_system_autotext: [],
    get_usage_stats: { autotextCounts: {} },
    get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
  });
}

describe("AutoText import and export", () => {
  beforeEach(() => {
    resetTauriMocks();
    vi.mocked(save).mockReset();
    vi.mocked(open).mockReset();
    vi.mocked(invoke).mockClear();
    vi.mocked(listen).mockResolvedValue(() => {});
    mockListenCapture();
    seed();
  });

  it("exports to the file the user picks and reports how many were written", async () => {
    vi.mocked(save).mockResolvedValue("/tmp/autotext.json");
    mockInvokeResponses({
      list_autotext: [ENTRY],
      list_system_autotext: [],
      get_usage_stats: { autotextCounts: {} },
      get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
      export_autotext: 1,
    });
    render(<AutoTextScreen />);

    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() =>
      expect(invokeCalls()).toContainEqual({
        cmd: "export_autotext",
        args: { path: "/tmp/autotext.json" },
      }),
    );
    expect(vi.mocked(save)).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: "autotext.json" }),
    );
    expect((await screen.findByRole("status")).textContent).toContain("Exported 1 snippet.");
  });

  it("does not call the backend when the export dialog is cancelled", async () => {
    vi.mocked(save).mockResolvedValue(null);
    render(<AutoTextScreen />);

    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(vi.mocked(save)).toHaveBeenCalled());
    expect(invokeCalls().some((c) => c.cmd === "export_autotext")).toBe(false);
  });

  it("imports the picked file, reports the counts and reloads the list", async () => {
    vi.mocked(open).mockResolvedValue("/tmp/autotext.json");
    mockInvokeResponses({
      list_autotext: [ENTRY, { ...ENTRY, id: "e2", trigger: "/phone", snippet: "my number" }],
      list_system_autotext: [],
      get_usage_stats: { autotextCounts: {} },
      get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
      import_autotext: { imported: 2, updated: 1, skipped: 0 },
    });
    render(<AutoTextScreen />);

    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    await waitFor(() =>
      expect(invokeCalls()).toContainEqual({
        cmd: "import_autotext",
        args: { path: "/tmp/autotext.json" },
      }),
    );
    expect((await screen.findByRole("status")).textContent).toContain(
      "Imported 2, updated 1, skipped 0.",
    );
    await waitFor(() =>
      expect(invokeCalls().filter((c) => c.cmd === "list_autotext")).toHaveLength(2),
    );
  });

  it("surfaces a rejected import instead of claiming success", async () => {
    vi.mocked(open).mockResolvedValue("/tmp/broken.json");
    mockInvokeResponses({
      list_autotext: [ENTRY],
      list_system_autotext: [],
      get_usage_stats: { autotextCounts: {} },
      get_settings: { typingAutotextEnabled: true, autotextTiming: "delimiter" },
    });
    render(<AutoTextScreen />);

    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    expect((await screen.findByRole("alert")).textContent).toContain(
      "unexpected invoke: import_autotext",
    );
    expect(screen.queryByRole("status")).toBeNull();
  });
});
