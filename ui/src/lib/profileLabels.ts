export const PROFILE_KEYS = ["clean", "natural", "structured", "transform", "off"] as const;

export type ProfileKey = (typeof PROFILE_KEYS)[number];

export interface ProfileDefinition {
  key: ProfileKey;
  label: string;
  promise: string;
  budgetPercent: number;
  budgetLabel: string;
}

export const PROFILES: readonly ProfileDefinition[] = [
  {
    key: "clean",
    label: "Clean",
    promise: "Fix only what is wrong. Keep my words.",
    budgetPercent: 15,
    budgetLabel: "Edit budget: up to 15% of words",
  },
  {
    key: "natural",
    label: "Natural",
    promise: "Make it correct and readable, still me.",
    budgetPercent: 30,
    budgetLabel: "Edit budget: up to 30% of words",
  },
  {
    key: "structured",
    label: "Structured",
    promise: "Make it ready to send.",
    budgetPercent: 45,
    budgetLabel: "Edit budget: up to 45% of words",
  },
  {
    key: "transform",
    label: "Transform",
    promise: "Rewrite it the way I asked.",
    budgetPercent: 100,
    budgetLabel: "Edit budget: no fixed ceiling",
  },
  {
    key: "off",
    label: "Off",
    promise: "No polish at all.",
    budgetPercent: 0,
    budgetLabel: "Edit budget: none",
  },
];

export function parseProfileKey(raw: string | null | undefined): ProfileKey {
  const key = (raw ?? "").trim().toLowerCase();
  return PROFILE_KEYS.find((k) => k === key) ?? "natural";
}

function definition(key: ProfileKey): ProfileDefinition {
  return PROFILES.find((p) => p.key === key) ?? PROFILES[1];
}

export function profileLabel(key: ProfileKey): string {
  return definition(key).label;
}

export function profilePromise(key: ProfileKey): string {
  return definition(key).promise;
}

export function profileBudgetPercent(key: ProfileKey): number {
  return definition(key).budgetPercent;
}

export function profileBudgetLabel(key: ProfileKey): string {
  return definition(key).budgetLabel;
}

export const DEFAULT_MAX_CHANGE_PERCENT = 40;

export function effectiveBudgetPercent(profile: ProfileKey, customPercent: number): number {
  if (profile === "off") return 0;
  if (profile === "transform") return 100;
  const budget = profileBudgetPercent(profile);
  return customPercent !== DEFAULT_MAX_CHANGE_PERCENT && customPercent < budget
    ? customPercent
    : budget;
}

export type PolishCustomization = "none" | "override" | "full";

export function polishCustomizationFor(modelId: string): PolishCustomization {
  if (modelId === "s1-mini" || modelId === "speakoflow-mini") return "none";
  if (modelId === "qwen3.5-2b" || modelId === "qwen3.5-0.8b") return "override";
  return "full";
}

export type ModelCapability = "cleanup-only" | "editor" | "instruction-capable";

export function modelCapabilityFor(modelId: string, reported?: string | null): ModelCapability {
  if (
    reported === "cleanup-only" ||
    reported === "editor" ||
    reported === "instruction-capable"
  ) {
    return reported;
  }
  return polishCustomizationFor(modelId) === "none" ? "cleanup-only" : "instruction-capable";
}

export function supportsProfile(capability: ModelCapability, key: ProfileKey): boolean {
  if (capability !== "cleanup-only") return true;
  return key === "clean" || key === "off";
}

export function profileList(keys: readonly ProfileKey[]): string {
  const labels = keys.map(profileLabel);
  if (labels.length <= 1) return labels[0] ?? "";
  if (labels.length === 2) return `${labels[0]} and ${labels[1]}`;
  return `${labels.slice(0, -1).join(", ")}, and ${labels[labels.length - 1]}`;
}

export function supportedProfileList(capability: ModelCapability): string {
  return profileList(PROFILE_KEYS.filter((k) => supportsProfile(capability, k)));
}

export function unsupportedProfileList(capability: ModelCapability): string {
  return profileList(PROFILE_KEYS.filter((k) => !supportsProfile(capability, k)));
}

export function effectiveProfileFor(stored: ProfileKey, capability: ModelCapability): ProfileKey {
  if (capability !== "cleanup-only") return stored;
  return stored === "natural" || stored === "structured" ? "clean" : stored;
}

export interface AppProfileRow {
  key: string;
  label: string;
  defaultProfile: ProfileKey | null;
  keys: readonly string[];
}

export const APP_PROFILE_ROWS: readonly AppProfileRow[] = [
  { key: "email", label: "Email", defaultProfile: "structured", keys: ["email"] },
  { key: "chat", label: "Chat", defaultProfile: "natural", keys: ["chat"] },
  { key: "ide", label: "IDE / Terminal", defaultProfile: "off", keys: ["coding", "terminal"] },
  { key: "document", label: "Docs", defaultProfile: "natural", keys: ["document", "notes"] },
  {
    key: "aichat",
    label: "AI chat",
    defaultProfile: "natural",
    keys: ["chatgpt", "com.openai.chatgpt", "claude", "cursor", "github.copilot"],
  },
  { key: "other", label: "Unknown", defaultProfile: null, keys: ["other"] },
];

export function rowStoredOverride(
  overrides: Record<string, string>,
  row: AppProfileRow,
): ProfileKey | null {
  for (const key of row.keys) {
    const raw = overrides[key];
    if (typeof raw === "string" && (PROFILE_KEYS as readonly string[]).includes(raw)) {
      return raw as ProfileKey;
    }
  }
  return null;
}

export function writeRowOverride(
  overrides: Record<string, string>,
  row: AppProfileRow,
  key: ProfileKey | "",
): Record<string, string> {
  const next = { ...overrides };
  for (const appKey of row.keys) {
    if (key === "") delete next[appKey];
    else next[appKey] = key;
  }
  return next;
}
