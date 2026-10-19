import { describe, it, expect } from 'vitest'
import { rainbowColor, RAINBOW } from '../lib/rainbow'

function hexToRgb(hex: string): [number, number, number] {
  const m = hex.match(/\w\w/g)!
  return m.map((h) => parseInt(h, 16)) as [number, number, number]
}

describe('rainbowColor', () => {
  it('returns first color at t=0', () => {
    const [r, g, b] = hexToRgb(RAINBOW[0])
    expect(rainbowColor(0)).toBe(`rgb(${r},${g},${b})`)
  })

  it('returns last color at t=1', () => {
    const [r, g, b] = hexToRgb(RAINBOW[RAINBOW.length - 1])
    expect(rainbowColor(1)).toBe(`rgb(${r},${g},${b})`)
  })

  it('clamps negative t to 0', () => {
    expect(rainbowColor(-1)).toBe(rainbowColor(0))
    expect(rainbowColor(-100)).toBe(rainbowColor(0))
  })

  it('clamps t>1 to 1', () => {
    expect(rainbowColor(2)).toBe(rainbowColor(1))
    expect(rainbowColor(100)).toBe(rainbowColor(1))
  })

  it('returns rgb string format', () => {
    expect(rainbowColor(0.5)).toMatch(/^rgb\(\d+,\d+,\d+\)$/)
  })

  it('interpolates at exact segment boundaries', () => {
    const n = RAINBOW.length - 1
    for (let i = 0; i < RAINBOW.length; i++) {
      const t = i / n
      const [r, g, b] = hexToRgb(RAINBOW[i])
      expect(rainbowColor(t)).toBe(`rgb(${r},${g},${b})`)
    }
  })

  it('is monotonic-ish: midpoint differs from endpoints', () => {
    const mid = rainbowColor(0.5)
    expect(mid).not.toBe(rainbowColor(0))
    expect(mid).not.toBe(rainbowColor(1))
  })

  it('is deterministic for same t', () => {
    expect(rainbowColor(0.33)).toBe(rainbowColor(0.33))
  })

  it('handles t=0.25 and t=0.75 without throwing', () => {
    expect(() => rainbowColor(0.25)).not.toThrow()
    expect(() => rainbowColor(0.75)).not.toThrow()
  })

  it('produces distinct colors for distinct t values', () => {
    const colors = [0, 0.2, 0.4, 0.6, 0.8, 1].map(rainbowColor)
    expect(new Set(colors).size).toBe(colors.length)
  })

  it('midpoint between first two rainbow colors is averaged', () => {
    const t = 0.5 / (RAINBOW.length - 1)
    const mid = rainbowColor(t)
    const [r0, g0, b0] = hexToRgb(RAINBOW[0])
    const [r1, g1, b1] = hexToRgb(RAINBOW[1])
    const er = Math.round((r0 + r1) / 2)
    const eg = Math.round((g0 + g1) / 2)
    const eb = Math.round((b0 + b1) / 2)
    expect(mid).toBe(`rgb(${er},${eg},${eb})`)
  })

  it('handles NaN by returning NaN rgb (documents current behavior)', () => {
    expect(() => rainbowColor(NaN)).toThrow()
  })
})
