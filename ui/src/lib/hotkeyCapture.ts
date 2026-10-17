// Webview hotkey capture for non-macOS platforms (plan P8.5a).
//
// macOS keeps the native capture panel (start_hotkey_capture /
// get_captured_hotkey in commands/settings.rs) because the webview KeyboardEvent
// API cannot see the Fn key at all. Everywhere else the app captures keydown /
// keyup on window and builds the chord from `event.code`, which is
// layout-stable (the physical key, not the character it produces).
//
// The chord is emitted in the same wire format the rest of the app persists:
// Tauri global-shortcut segments joined by "+", modifiers first in the fixed
// order Shift / Control / Alt / Command, then the main key
// ("Ctrl+Shift+Space"). That is exactly what `save_settings` /
// `update_settings_patch` accept for `hotkey`, `cancelHotkey` and
// `quickAddHotkey`, so no new IPC is needed to store a captured chord.

export type ModifierName = "Shift" | "Control" | "Alt" | "Command";

/** Display order of modifier segments in the emitted hotkey string. */
export const MODIFIER_ORDER: readonly ModifierName[] = [
  "Shift",
  "Control",
  "Alt",
  "Command",
];

const MODIFIER_CODES: Readonly<Record<string, ModifierName>> = {
  ShiftLeft: "Shift",
  ShiftRight: "Shift",
  ControlLeft: "Control",
  ControlRight: "Control",
  AltLeft: "Alt",
  AltRight: "Alt",
  MetaLeft: "Command",
  MetaRight: "Command",
};

/**
 * Maps a KeyboardEvent `code` to the Tauri global-shortcut key segment
 * ("Space", "F1", "A", "Minus", …). Returns null for codes that are
 * modifiers or that the global-shortcut parser cannot bind (Dead keys,
 * Numpad Enter, Context/Menu, …).
 */
export function codeToKey(code: string): string | null {
  if (MODIFIER_CODES[code] !== undefined) return null;
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  switch (code) {
    case "Space":
      return "Space";
    case "Minus":
      return "Minus";
    case "Equal":
      return "Equal";
    case "BracketLeft":
      return "BracketLeft";
    case "BracketRight":
      return "BracketRight";
    case "Backslash":
      return "Backslash";
    case "Semicolon":
      return "Semicolon";
    case "Quote":
      return "Quote";
    case "Comma":
      return "Comma";
    case "Period":
      return "Period";
    case "Slash":
      return "Slash";
    case "Backquote":
      return "Backquote";
    case "Enter":
      return "Enter";
    case "Escape":
      return "Escape";
    case "Tab":
      return "Tab";
    case "Backspace":
      return "Backspace";
    case "Insert":
      return "Insert";
    case "Delete":
      return "Delete";
    case "Home":
      return "Home";
    case "End":
      return "End";
    case "PageUp":
      return "PageUp";
    case "PageDown":
      return "PageDown";
    case "ArrowUp":
      return "ArrowUp";
    case "ArrowDown":
      return "ArrowDown";
    case "ArrowLeft":
      return "ArrowLeft";
    case "ArrowRight":
      return "ArrowRight";
    case "CapsLock":
      return "CapsLock";
    case "NumLock":
      return "NumLock";
    case "ScrollLock":
      return "ScrollLock";
    default: {
      const f = /^F([1-9]|1[0-3])$/.exec(code);
      if (f) return code;
      return null;
    }
  }
}

/** True when the code is one of the four chord modifiers. */
export function isModifierCode(code: string): boolean {
  return MODIFIER_CODES[code] !== undefined;
}

export interface ChordState {
  /** Modifier names in press order (order is irrelevant; display sorts). */
  mods: ModifierName[];
  /** Main key segment, null while still waiting for a key. */
  key: string | null;
}

/** A fresh, empty capture. */
export function initialChord(): ChordState {
  return { mods: [], key: null };
}

/** Human-friendly name of a physical key for the in-progress display. */
export function codeToLabel(code: string): string {
  const mod = MODIFIER_CODES[code];
  if (mod) return mod;
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  const names: Readonly<Record<string, string>> = {
    Space: "Space",
    Enter: "Enter",
    Escape: "Esc",
    Tab: "Tab",
    Backspace: "Bksp",
    Backquote: "`",
    Minus: "-",
    Equal: "=",
    BracketLeft: "[",
    BracketRight: "]",
    Backslash: "\\",
    Semicolon: ";",
    Quote: "'",
    Comma: ",",
    Period: ".",
    Slash: "/",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
  };
  return names[code] ?? code;
}

interface KeydownInput {
  code: string;
  repeat: boolean;
}

/**
 * Feeds a keydown into the chord. Returns the next state, or the same state
 * when the event is ignored (repeat, unsupported code, already-pressed
 * modifier, or a modifier arriving after the main key).
 */
export function chordOnKeydown(state: ChordState, e: KeydownInput): ChordState {
  if (e.repeat || state.key !== null) return state;
  const mod = MODIFIER_CODES[e.code];
  if (mod) {
    if (state.mods.includes(mod)) return state;
    return { ...state, mods: [...state.mods, mod] };
  }
  if (e.code === "Escape") {
    // Escape always cancels a capture, even as the main key. The caller
    // treats the returned state as "cancelled" (see chordResult).
    return initialChord();
  }
  const key = codeToKey(e.code);
  if (key === null) return state;
  return { ...state, key };
}

/** Feeds a keyup. Only the chord modifiers it owns are removed. */
export function chordOnKeyup(state: ChordState, code: string): ChordState {
  const mod = MODIFIER_CODES[code];
  if (!mod || !state.mods.includes(mod)) return state;
  return { ...state, mods: state.mods.filter((m) => m !== mod) };
}

/**
 * The chord so far, as an in-progress display string ("⇧ ⌃ + …").
 * Used by the recorder while it is waiting for the main key.
 */
export function chordDisplay(state: ChordState): string {
  const parts = MODIFIER_ORDER.filter((m) => state.mods.includes(m)).map(
    (m) => ({ Shift: "⇧", Control: "⌃", Alt: "⌥", Command: "⌘" })[m],
  );
  if (state.key) parts.push(state.key);
  return parts.length > 0 ? parts.join(" ") : "";
}

/**
 * The finished hotkey string in wire format: modifiers in the fixed
 * Shift/Control/Alt/Command order, then the main key.
 */
export function chordToHotkey(state: ChordState): string {
  const mods = MODIFIER_ORDER.filter((m) => state.mods.includes(m));
  if (state.key === null) return mods.join("+");
  return [...mods, state.key].join("+");
}

/**
 * Outcome of a capture. `"cancelled"` when the user pressed Escape (or the
 * chord was empty), `"committed"` with the wire-format hotkey otherwise.
 */
export type ChordResult =
  | { status: "committed"; hotkey: string }
  | { status: "cancelled" };

/**
 * Resolves a capture once no keys are held: an empty chord is a cancel,
 * otherwise the chord is committed. `state` is the state *after* the final
 * keyup(s), so a modifier-only press that was released resolves to cancelled.
 */
export function chordResult(state: ChordState): ChordResult {
  if (state.key === null && state.mods.length === 0) {
    return { status: "cancelled" };
  }
  return { status: "committed", hotkey: chordToHotkey(state) };
}

/**
 * Platform gate: the webview recorder runs only where the native capture
 * panel cannot (macOS keeps the Obj-C panel, which is the only path that can
 * see the Fn key). No existing platform util in ui/src, so this uses
 * navigator.platform; the userAgentData check covers Chromium builds that
 * report a generic platform.
 */
interface UserAgentDataLite {
  platform?: string;
}

function readAgentPlatform(): string | null {
  if (typeof navigator === "undefined") return null;
  const nav = navigator as Navigator & { userAgentData?: UserAgentDataLite };
  return typeof nav.userAgentData?.platform === "string"
    ? nav.userAgentData.platform
    : null;
}

export function isMacOS(): boolean {
  if (typeof navigator === "undefined") return false;
  if (/^Mac/i.test((navigator as Navigator & { platform: string }).platform)) {
    return true;
  }
  return readAgentPlatform() === "macOS";
}
