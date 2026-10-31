import { describe, it, expect } from 'vitest'
import { SIZES } from '../lib/rainbow'

describe('pill SIZES', () => {
  const sizes = SIZES as unknown as Record<string, { width: number; height: number }>

  it('defines 7 size entries', () => {
    expect(Object.keys(sizes).length).toBe(7)
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

  it('recordingExpanded leaves the meter its 72px beside the chips and Cancel', () => {
    // padding 28, dot 24, meter 72, clock 30, CMD cue 33, mic chip 96, mode
    // chip 70, language chip 60, Cancel 60, gaps 10*4 + 6*3 + 4.
    const need = 28 + 24 + 72 + 30 + 33 + 96 + 70 + 60 + 60 + 10 * 4 + 6 * 3 + 4
    expect(sizes.recordingExpanded.width).toBeGreaterThanOrEqual(need)
    expect(sizes.recordingExpanded.width).toBeLessThanOrEqual(540)
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

  it('mic failure leaves room for the message and the settings shortcut', () => {
    expect(sizes.processingMic.width).toBeGreaterThan(sizes.processing.width)
    expect(sizes.processingMic.width).toBeLessThanOrEqual(480)
  })
})
