// claim: ui-behavior
/**
 * The webview recorder (non-macOS hotkey capture) must behave like the native
 * macOS panel: Esc cancels the capture without changing the binding, and a
 * real key press commits the chord in the shared wire format.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import WebviewHotkeyRecorder from "../components/WebviewHotkeyRecorder";

afterEach(() => {
  cleanup();
});

function renderRecorder(onSave = vi.fn()) {
  render(<WebviewHotkeyRecorder value="Ctrl+Shift+Space" onSave={onSave} />);
  return { onSave };
}

describe("WebviewHotkeyRecorder", () => {
  it("starts capturing when the keycap is tapped", () => {
    renderRecorder();
    fireEvent.pointerDown(screen.getByRole("button", { name: /hotkey/i }));
    expect(screen.getByText("Press keys…")).toBeTruthy();
  });

  it("Esc cancels the capture without saving", async () => {
    const { onSave } = renderRecorder();
    fireEvent.pointerDown(screen.getByRole("button", { name: /hotkey/i }));
    await screen.findByText("Press keys…");

    // Before the fix, Esc only reset the in-progress chord and the recorder
    // kept capturing, contradicting the "Esc cancels" hint under the keycap
    // and the native panel's behaviour.
    fireEvent.keyDown(window, { code: "Escape" });

    await waitFor(() => expect(screen.queryByText("Press keys…")).toBeNull());
    expect(onSave).not.toHaveBeenCalled();
    // The keycap is back to showing the bound value, not an empty chord.
    expect(screen.getByRole("button", { name: /hotkey/i }).textContent).toContain("Ctrl");
  });

  it("commits a chord in the shared wire format on a real key press", async () => {
    const { onSave } = renderRecorder();
    fireEvent.pointerDown(screen.getByRole("button", { name: /hotkey/i }));
    await screen.findByText("Press keys…");

    fireEvent.keyDown(window, { code: "ControlLeft" });
    fireEvent.keyDown(window, { code: "ShiftLeft" });
    fireEvent.keyDown(window, { code: "Space" });

    await waitFor(() => expect(onSave).toHaveBeenCalledWith("Shift+Control+Space"));
  });
});
