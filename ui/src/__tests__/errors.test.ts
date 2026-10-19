import { describe, it, expect } from 'vitest'
import { errorMessage, errorCode } from '../lib/errors'

describe('errorMessage', () => {
  it('passes strings through', () => {
    expect(errorMessage('boom')).toBe('boom')
  })

  it('prefers the structured CommandError message over [object Object]', () => {
    expect(errorMessage({ code: 'E_NOT_FOUND', message: 'Word not found' })).toBe('Word not found')
  })

  it('falls back to String() for exotic values', () => {
    expect(errorMessage(42)).toBe('42')
    expect(errorMessage(null)).toBe('null')
    expect(errorMessage({})).toBe('[object Object]')
  })
})

describe('errorCode', () => {
  it('extracts the backend code', () => {
    expect(errorCode({ code: 'E_MODEL', message: 'x' })).toBe('E_MODEL')
  })

  it('returns null when there is no code', () => {
    expect(errorCode('plain string')).toBeNull()
    expect(errorCode(null)).toBeNull()
  })
})
