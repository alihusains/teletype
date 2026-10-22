// claim: ui-behavior
import { describe, it, expect } from 'vitest'
import { bucketDaily, statFontSize } from '../screens/InsightsScreen'

function day(i: number, words: number, dictations: number) {
  return { label: `Day ${i}`, date: i, words, dictations }
}

describe('bucketDaily', () => {
  it('buckets 365 days into <= 22 bars and preserves totals', () => {
    const daily = Array.from({ length: 365 }, (_, i) => day(i, 1, 2))
    const out = bucketDaily(daily, 22)
    expect(out.length).toBeLessThanOrEqual(22)
    expect(out.reduce((s, d) => s + d.words, 0)).toBe(365)
    expect(out.reduce((s, d) => s + d.dictations, 0)).toBe(730)
    // each bucket labels from its first day
    expect(out[0].label).toBe('Day 0')
    expect(out[1].label).toBe('Day 17')
  })

  it('is a no-op when length <= maxBars', () => {
    const daily = Array.from({ length: 22 }, (_, i) => day(i, 3, 1))
    const out = bucketDaily(daily, 22)
    expect(out).toBe(daily)
  })

  it('buckets uneven counts without losing data', () => {
    const daily = Array.from({ length: 50 }, (_, i) => day(i, i + 1, 1))
    const out = bucketDaily(daily, 22)
    expect(out.length).toBeLessThanOrEqual(22)
    expect(out.reduce((s, d) => s + d.words, 0)).toBe(daily.reduce((s, d) => s + d.words, 0))
    expect(out.reduce((s, d) => s + d.dictations, 0)).toBe(50)
  })
})

describe('statFontSize', () => {
  it('scales down as the value grows', () => {
    expect(statFontSize('123456')).toBe(30)
    expect(statFontSize('1,000,000')).toBe(24)
    expect(statFontSize('12,345,678')).toBe(24)
    expect(statFontSize('123,456,789')).toBe(20)
    expect(statFontSize('1,234,567,890')).toBe(20)
    expect(statFontSize('12,345,678,901')).toBe(17)
  })
})
