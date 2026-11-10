// claim: ui-behavior
//
// The agentic polish indicator: while the take is being polished the pill is
// in the "Transforming…" (streaming) phase and shows a pulsing sparkle cue.
// The "Transcribing…" (transcribing) phase must not show it, so the cue reads
// as "the model is actively rewriting the take", not "something is busy".
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, screen } from "@testing-library/react";
import { resetTauriMocks } from "./tauriMock";
import { renderPill, resetPillHarness, setPhase } from "./pillHarness";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

afterEach(() => {
  cleanup();
  resetTauriMocks();
  resetPillHarness();
});

describe("agentic polish indicator", () => {
  it("shows the agentic cue during the Transforming (polish) stage", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Transforming…" });
    await screen.findByText("Transforming…");
    expect(document.querySelector("[data-agentic-cue]")).toBeTruthy();
  });

  it("does not show the agentic cue during the Transcribing stage", async () => {
    await renderPill("default");
    setPhase({ phase: "processing", message: "Transcribing…" });
    await screen.findByText("Transcribing…");
    expect(document.querySelector("[data-agentic-cue]")).toBeNull();
  });
});
