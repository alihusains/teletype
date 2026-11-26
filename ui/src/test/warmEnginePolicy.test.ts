// claim: ui-behavior
/**
 * D052: the model is never unloaded automatically, so the pre-warm control no
 * longer offers retention windows. A config that still carries a legacy "10" or
 * "60" window must still show a selected choice, not an empty segmented control.
 */
import { describe, expect, it } from "vitest";
import { warmPolicyChoice } from "../settings/MicrophoneSoundsSections";

describe("pre-warm engine control", () => {
  it("shows legacy retention windows as the 'on first press' choice", () => {
    expect(warmPolicyChoice("10")).toBe("30");
    expect(warmPolicyChoice("30")).toBe("30");
    expect(warmPolicyChoice("60")).toBe("30");
    expect(warmPolicyChoice("")).toBe("30");
  });

  it("keeps the two named choices as they are", () => {
    expect(warmPolicyChoice("off")).toBe("off");
    expect(warmPolicyChoice("always")).toBe("always");
  });
});
