// @vitest-environment jsdom
import { describe, it, expect, afterAll } from 'vitest'
import {
  codeToKey,
  isModifierCode,
  initialChord,
  chordOnKeydown,
  chordOnKeyup,
  chordDisplay,
  chordToHotkey,
  chordResult,
  isMacOS,
  type ChordState,
} from '../lib/hotkeyCapture'

describe('codeToKey', () => {
  it('maps letter codes to letters', () => {
    expect(codeToKey('KeyA')).toBe('A')
    expect(codeToKey('KeyZ')).toBe('Z')
  })

  it('maps digit codes to digits', () => {
    expect(codeToKey('Digit1')).toBe('1')
    expect(codeToKey('Digit0')).toBe('0')
  })

  it('maps special keys to Tauri segments', () => {
    expect(codeToKey('Space')).toBe('Space')
    expect(codeToKey('Escape')).toBe('Escape')
    expect(codeToKey('Enter')).toBe('Enter')
    expect(codeToKey('Tab')).toBe('Tab')
    expect(codeToKey('F12')).toBe('F12')
    expect(codeToKey('ArrowUp')).toBe('ArrowUp')
    expect(codeToKey('Minus')).toBe('Minus')
    expect(codeToKey('Period')).toBe('Period')
  })

  it('returns null for modifier codes', () => {
    expect(codeToKey('ShiftLeft')).toBeNull()
    expect(codeToKey('ControlRight')).toBeNull()
    expect(codeToKey('AltLeft')).toBeNull()
    expect(codeToKey('MetaLeft')).toBeNull()
  })

  it('returns null for unbindable keys', () => {
    expect(codeToKey('DeadGrave')).toBeNull()
    expect(codeToKey('NumpadEnter')).toBeNull()
    expect(codeToKey('ContextMenu')).toBeNull()
    expect(codeToKey('F14')).toBeNull()
  })
})

describe('isModifierCode', () => {
  it('is true for all four modifier codes', () => {
    expect(isModifierCode('ShiftLeft')).toBe(true)
    expect(isModifierCode('ShiftRight')).toBe(true)
    expect(isModifierCode('ControlLeft')).toBe(true)
    expect(isModifierCode('ControlRight')).toBe(true)
    expect(isModifierCode('AltLeft')).toBe(true)
    expect(isModifierCode('AltRight')).toBe(true)
    expect(isModifierCode('MetaLeft')).toBe(true)
    expect(isModifierCode('MetaRight')).toBe(true)
  })

  it('is false for regular keys', () => {
    expect(isModifierCode('KeyA')).toBe(false)
    expect(isModifierCode('Space')).toBe(false)
    expect(isModifierCode('Escape')).toBe(false)
  })
})

describe('chord building', () => {
  it('starts empty', () => {
    expect(chordToHotkey(initialChord())).toBe('')
    expect(chordDisplay(initialChord())).toBe('')
  })

  it('accumulates modifiers in press order, deduplicated', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'ControlLeft', repeat: false })
    s = chordOnKeydown(s, { code: 'ShiftLeft', repeat: false })
    s = chordOnKeydown(s, { code: 'ShiftRight', repeat: false })
    expect(s.mods).toEqual(['Control', 'Shift'])
  })

  it('ignores a modifier pressed after the main key', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'KeyA', repeat: false })
    s = chordOnKeydown(s, { code: 'ShiftLeft', repeat: false })
    expect(s).toEqual({ mods: [], key: 'A' })
  })

  it('ignores key repeat', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'ShiftLeft', repeat: false })
    const repeated = chordOnKeydown(s, { code: 'ShiftLeft', repeat: true })
    expect(repeated).toBe(s)
  })

  it('ignores unbindable keys', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'ControlLeft', repeat: false })
    const next = chordOnKeydown(s, { code: 'DeadGrave', repeat: false })
    expect(next).toBe(s)
  })

  it('commits on a non-modifier key', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'ControlLeft', repeat: false })
    s = chordOnKeydown(s, { code: 'ShiftLeft', repeat: false })
    s = chordOnKeydown(s, { code: 'Space', repeat: false })
    expect(s.key).toBe('Space')
    expect(chordResult(s)).toEqual({ status: 'committed', hotkey: 'Shift+Control+Space' })
  })

  it('removes modifiers on keyup', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'ControlLeft', repeat: false })
    s = chordOnKeydown(s, { code: 'ShiftLeft', repeat: false })
    s = chordOnKeyup(s, 'ControlLeft')
    expect(s.mods).toEqual(['Shift'])
    // A keyup for a key not in the chord is a no-op.
    expect(chordOnKeyup(s, 'KeyA')).toBe(s)
  })

  it('resolves an empty chord as cancelled', () => {
    expect(chordResult(initialChord()).status).toBe('cancelled')
  })

  it('resolves a modifier-only released chord as cancelled', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'ControlLeft', repeat: false })
    s = chordOnKeyup(s, 'ControlLeft')
    expect(chordResult(s).status).toBe('cancelled')
  })
})

describe('modifier ordering in wire format', () => {
  function chordWith(mods: ChordState['mods']): ChordState {
    return { mods, key: 'Space' }
  }

  it('emits Shift+Control+Alt+Command order regardless of press order', () => {
    expect(chordToHotkey(chordWith(['Command', 'Alt', 'Control', 'Shift']))).toBe('Shift+Control+Alt+Command+Space')
  })

  it('matches the Rust default for Windows', () => {
    expect(chordToHotkey(chordWith(['Control', 'Shift']))).toBe('Shift+Control+Space')
  })

  it('emits a bare key with no modifiers', () => {
    expect(chordToHotkey({ mods: [], key: 'F1' })).toBe('F1')
  })
})

describe('chordDisplay (in-progress)', () => {
  it('shows modifiers with symbols in fixed order', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'MetaLeft', repeat: false })
    s = chordOnKeydown(s, { code: 'ShiftLeft', repeat: false })
    expect(chordDisplay(s)).toBe('⇧ ⌘')
  })

  it('includes the main key once committed', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'ControlLeft', repeat: false })
    s = chordOnKeydown(s, { code: 'KeyA', repeat: false })
    expect(chordDisplay(s)).toBe('⌃ A')
  })
})

describe('Escape', () => {
  it('cancels the capture even with modifiers held', () => {
    let s = initialChord()
    s = chordOnKeydown(s, { code: 'ControlLeft', repeat: false })
    s = chordOnKeydown(s, { code: 'Escape', repeat: false })
    expect(chordResult(s).status).toBe('cancelled')
  })

  it('Escape as the first key cancels', () => {
    const s = chordOnKeydown(initialChord(), { code: 'Escape', repeat: false })
    expect(chordResult(s).status).toBe('cancelled')
  })
})

describe('isMacOS platform gate', () => {
  const originalDescriptor = Object.getOwnPropertyDescriptor(navigator, 'platform')
  const originalUad = Object.getOwnPropertyDescriptor(navigator, 'userAgentData')

  function setPlatform(value: string | undefined) {
    Object.defineProperty(navigator, 'platform', { value, configurable: true })
  }

  function setUadPlatform(value: string | undefined) {
    Object.defineProperty(navigator, 'userAgentData', {
      value: { platform: value },
      configurable: true,
    })
  }

  it.each([
    ['MacIntel', true],
    ['Macintosh', true],
    ['Win32', false],
    ['Linux x86_64', false],
  ])('platform %s → %s', (platform, expected) => {
    setPlatform(platform)
    Object.defineProperty(navigator, 'userAgentData', { value: undefined, configurable: true })
    expect(isMacOS()).toBe(expected)
  })

  it('falls back to userAgentData when navigator.platform is generic', () => {
    setPlatform('')
    setUadPlatform('macOS')
    expect(isMacOS()).toBe(true)
  })

  it('is false when both sources are absent', () => {
    setPlatform('')
    Object.defineProperty(navigator, 'userAgentData', { value: undefined, configurable: true })
    expect(isMacOS()).toBe(false)
  })

  afterAll(() => {
    if (originalDescriptor) Object.defineProperty(navigator, 'platform', originalDescriptor)
    if (originalUad) Object.defineProperty(navigator, 'userAgentData', originalUad)
  })
})

