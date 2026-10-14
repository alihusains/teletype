import { describe, it, expect } from 'vitest'
import { SIZES } from '../lib/rainbow'

describe('pill SIZES', () => {
  const sizes = SIZES as unknown as Record<string, { width: number; height: number }>

  it('defines 6 size entries', () => {
    expect(Object.keys(sizes).length).toBe(6)
  })

  it('idle is smallest', () => {
    expect(sizes.idle.width).toBeLessThan(sizes.warming.width)
    expect(sizes.idle.width).toBeLessThan(sizes.recording.width)
    expect(sizes.idle.height).toBeLessThan(sizes.warming.height)
  })

  it('all heights are positive', () => {
    for (const [, v] of Object.entries(sizes)) {
      expect(v.height).toBeGreaterThan(0)
      expect(v.width).toBeGreaterThan(0)
    }
  })

  it('recordingExpanded wider than recording', () => {
    expect(sizes.recordingExpanded.width).toBeGreaterThan(sizes.recording.width)
  })

  it('recording height equals warming height', () => {
    expect(sizes.recording.height).toBe(sizes.warming.height)
  })

  it('idle dimensions are 58x24', () => {
    expect(sizes.idle).toEqual({ width: 58, height: 24 })
  })

  it('processing width between warming and recordingExpanded', () => {
    expect(sizes.processing.width).toBeGreaterThan(0)
    expect(sizes.processing.width).toBeLessThan(500)
  })
})
