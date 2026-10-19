import { describe, it, expect } from 'vitest'
import { displayHotkey } from '../components/HotkeyRecorder'

describe('displayHotkey', () => {
  it('returns em dash for empty string', () => {
    expect(displayHotkey('')).toBe('—')
  })

  it('maps Cmd to ⌘', () => {
    expect(displayHotkey('Cmd+Space')).toBe('⌘ Space')
  })

  it('maps Ctrl to ⌃', () => {
    expect(displayHotkey('Ctrl+Shift+A')).toBe('⌃ ⇧ A')
  })

  it('maps Alt to ⌥', () => {
    expect(displayHotkey('Alt+1')).toBe('⌥ 1')
  })

  it('maps Shift to ⇧', () => {
    expect(displayHotkey('Shift+A')).toBe('⇧ A')
  })

  it('maps Fn to 🌐', () => {
    expect(displayHotkey('Fn+A')).toBe('🌐 A')
  })

  it('maps Space segment to Space word', () => {
    expect(displayHotkey('Cmd+Space')).toBe('⌘ Space')
  })

  it('leaves unknown segments as-is', () => {
    expect(displayHotkey('Cmd+K')).toBe('⌘ K')
    expect(displayHotkey('Ctrl+F12')).toBe('⌃ F12')
  })

  it('handles single key without plus', () => {
    expect(displayHotkey('F1')).toBe('F1')
    expect(displayHotkey('Cmd')).toBe('⌘')
  })

  it('handles typical Teletype hotkey', () => {
    expect(displayHotkey('Alt+Space')).toBe('⌥ Space')
  })

  it('joins mapped segments with space', () => {
    const result = displayHotkey('Ctrl+Alt+Shift+A')
    expect(result).toBe('⌃ ⌥ ⇧ A')
  })
})
