// claim: ui-behavior
import { describe, it, expect } from 'vitest'
import { changeCount, diffWords, formatPolishMs } from '../lib/wordDiff'

describe('word diff for the polish card', () => {
  it('reports no change for identical text', () => {
    const segments = diffWords('ship it today', 'ship it today')
    expect(changeCount(segments)).toBe(0)
    expect(segments).toEqual([{ kind: 'same', text: 'ship it today' }])
  })

  it('counts a replacement as one change', () => {
    const segments = diffWords('we ship today', 'we launch today')
    expect(changeCount(segments)).toBe(1)
    expect(segments.filter((s) => s.kind === 'del').map((s) => s.text)).toContain('ship')
    expect(segments.filter((s) => s.kind === 'add').map((s) => s.text)).toContain('launch')
  })

  it('counts insertions and deletions separately', () => {
    expect(changeCount(diffWords('ship today', 'ship the build today'))).toBe(1)
    expect(changeCount(diffWords('ship the build today', 'ship today'))).toBe(1)
  })

  it('counts two separated edits as two changes', () => {
    const segments = diffWords('so we ship later maybe', 'We ship now maybe')
    expect(changeCount(segments)).toBeGreaterThanOrEqual(2)
  })

  it('handles an empty side', () => {
    const added = diffWords('', 'hello there')
    expect(added).toEqual([{ kind: 'add', text: 'hello there' }])
    expect(changeCount(added)).toBe(1)

    const removed = diffWords('hello there', '')
    expect(removed).toEqual([{ kind: 'del', text: 'hello there' }])
    expect(changeCount(removed)).toBe(1)
  })

  it('merges consecutive runs so a full rewrite is one span', () => {
    const segments = diffWords('one two three', 'four five six')
    const dels = segments.filter((s) => s.kind === 'del')
    expect(dels).toHaveLength(1)
    expect(dels[0].text).toContain('one two three')
  })

  it('falls back to a coarse pair for very long takes', () => {
    const before = Array.from({ length: 420 }, (_, i) => `a${i}`).join(' ')
    const after = Array.from({ length: 420 }, (_, i) => `b${i}`).join(' ')
    const segments = diffWords(before, after)
    expect(segments).toHaveLength(2)
    expect(changeCount(segments)).toBe(1)
  })

  it('formats durations to one decimal place', () => {
    expect(formatPolishMs(840)).toBe('0.8s')
    expect(formatPolishMs(1640)).toBe('1.6s')
    expect(formatPolishMs(0)).toBe('')
  })
})
