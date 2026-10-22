// claim: ui-behavior
import { describe, it, expect } from 'vitest'
import { countWords, measureWords } from '../screens/StylesScreen'

/**
 * Unit H: the Style screen has to make a style measurable. These are the two
 * numbers the "See the difference" band prints under each polished result, so
 * they are the parts worth pinning down.
 */
describe('countWords', () => {
  it('counts runs of whitespace as one separator', () => {
    expect(countWords('be concise and warm')).toBe(4)
    expect(countWords('  a\t\tb\n c  ')).toBe(3)
  })

  it('is zero for empty and whitespace-only text', () => {
    expect(countWords('')).toBe(0)
    expect(countWords('   \n\t ')).toBe(0)
  })
})

describe('measureWords', () => {
  const seed =
    'hey so i was thinking maybe we could possibly try to circle back on the thing from yesterday when you get a chance no rush at all'

  it('reports a shorter output as a percentage below the original', () => {
    // 27 words in, 7 out -> 74% shorter.
    const m = measureWords(seed, 'circle back on yesterday, when you can')
    expect(countWords(seed)).toBe(27)
    expect(m.words).toBe(7)
    expect(m.percent).toBe(74)
    expect(m.label).toBe('shorter by 74%')
  })

  it('reports a longer output in the other direction', () => {
    const long = seed + ' and let us also cover the week before that one'
    const m = measureWords(seed, long)
    expect(m.percent).toBeLessThan(0)
    expect(m.label).toBe(`longer by ${Math.abs(m.percent)}%`)
  })

  it('calls an identical result the same length', () => {
    const m = measureWords(seed, seed)
    expect(m.percent).toBe(0)
    expect(m.label).toBe('same length as the original')
  })

  it('does not divide by zero when the original is empty', () => {
    const m = measureWords('', 'some words')
    expect(m.words).toBe(2)
    expect(m.percent).toBe(0)
    expect(m.label).toBe('no original to measure against')
  })

  it('rounds to whole percents so the two panes stay comparable', () => {
    // 3 words -> 2 words is a third shorter: 33%, not 33.33333%.
    expect(measureWords('a b c', 'a b').label).toBe('shorter by 33%')
  })
})
