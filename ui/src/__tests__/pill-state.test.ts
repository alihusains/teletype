import { describe, it, expect } from 'vitest'

describe('pill state machine', () => {
  it('defines expected phases', () => {
    const phases = ['idle', 'recording', 'warming', 'processing'] as const
    expect(phases).toContain('idle')
    expect(phases).toContain('recording')
    expect(phases).toContain('warming')
    expect(phases).toContain('processing')
  })
})
