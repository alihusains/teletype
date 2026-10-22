// claim: ui-behavior
import { describe, it, expect } from 'vitest'
import { displayLevel, pillReducer, initialPillMachineState } from '../pillMachine'

describe('displayLevel', () => {
  it('maps the endpoints exactly', () => {
    expect(displayLevel(0)).toBe(0)
    expect(displayLevel(1)).toBe(1)
  })

  it('is monotonic', () => {
    const samples = [0, 0.1, 0.25, 0.4, 0.5, 0.75, 0.9, 1]
    const out = samples.map(displayLevel)
    for (let i = 1; i < out.length; i++) {
      expect(out[i]).toBeGreaterThanOrEqual(out[i - 1])
    }
  })

  it('lifts quiet speech so meters stay readable at low mic gain', () => {
    // Typical near-field speech through a low macOS input volume lands
    // around 0.2..0.5; it must not render as a flat line.
    expect(displayLevel(0.25)).toBeGreaterThanOrEqual(0.5)
    expect(displayLevel(0.5)).toBeGreaterThanOrEqual(0.7)
  })

  it('clamps out-of-range input', () => {
    expect(displayLevel(-0.3)).toBe(0)
    expect(displayLevel(1.7)).toBe(1)
  })
})

describe('pillReducer level action', () => {
  it('pushes the display-scaled level into the history', () => {
    const s = pillReducer(initialPillMachineState(), { type: 'level', level: 0.25 })
    const last = s.levels[s.levels.length - 1]
    expect(last).toBeCloseTo(displayLevel(0.25))
  })
})
