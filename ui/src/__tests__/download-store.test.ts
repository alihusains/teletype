import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}))

import {
  isDownloading,
  getProgress,
  markDownloading,
  clearDownload,
  snapshotDownloads,
  subscribeDownloads,
  loadAcceptedLicenses,
  saveAcceptedLicense,
} from '../lib/downloadStore'

describe('downloadStore', () => {
  beforeEach(() => {
    // clear any residual state via clearDownload
    // snapshotDownloads returns active ids — clear them all
    const { active } = snapshotDownloads()
    for (const id of [...active]) clearDownload(id)
    // also clear progress map entries
    const { progress } = snapshotDownloads()
    for (const k of Object.keys(progress)) clearDownload(k)
    localStorage.clear()
  })

  afterEach(() => {
    localStorage.clear()
  })

  it('markDownloading makes isDownloading true', () => {
    markDownloading('m1')
    expect(isDownloading('m1')).toBe(true)
  })

  it('clearDownload removes downloading state', () => {
    markDownloading('m1')
    clearDownload('m1')
    expect(isDownloading('m1')).toBe(false)
  })

  it('isDownloading false for unknown id', () => {
    expect(isDownloading('nope')).toBe(false)
  })

  it('getProgress returns undefined for unknown id', () => {
    expect(getProgress('nope')).toBeUndefined()
  })

  it('subscribeDownloads is notified on markDownloading', () => {
    const fn = vi.fn()
    const unsub = subscribeDownloads(fn)
    markDownloading('x')
    expect(fn).toHaveBeenCalled()
    unsub()
  })

  it('subscribeDownloads unsubscribe stops notifications', () => {
    const fn = vi.fn()
    const unsub = subscribeDownloads(fn)
    unsub()
    fn.mockClear()
    markDownloading('y')
    expect(fn).not.toHaveBeenCalled()
  })

  it('snapshotDownloads returns stable reference between no-op calls', () => {
    markDownloading('a')
    const s1 = snapshotDownloads()
    const s2 = snapshotDownloads()
    expect(s1).toBe(s2)
  })

  it('snapshotDownloads invalidates cache after mutation', () => {
    markDownloading('a')
    const s1 = snapshotDownloads()
    markDownloading('b')
    const s2 = snapshotDownloads()
    expect(s1).not.toBe(s2)
    expect(s2.active).toContain('b')
  })

  it('snapshotDownloads active contains marked ids', () => {
    markDownloading('m1')
    markDownloading('m2')
    const { active } = snapshotDownloads()
    expect(active).toContain('m1')
    expect(active).toContain('m2')
  })

  it('clearDownload removes from snapshot active', () => {
    markDownloading('m1')
    clearDownload('m1')
    expect(snapshotDownloads().active).not.toContain('m1')
  })

  it('loadAcceptedLicenses returns empty object when nothing stored', () => {
    expect(loadAcceptedLicenses()).toEqual({})
  })

  it('saveAcceptedLicense and loadAcceptedLicenses roundtrip', () => {
    saveAcceptedLicense('model-a', true)
    expect(loadAcceptedLicenses()).toEqual({ 'model-a': true })
  })

  it('saveAcceptedLicense false removes entry', () => {
    saveAcceptedLicense('model-a', true)
    saveAcceptedLicense('model-a', false)
    expect(loadAcceptedLicenses()).toEqual({})
  })

  it('saveAcceptedLicense handles multiple licenses', () => {
    saveAcceptedLicense('a', true)
    saveAcceptedLicense('b', true)
    const loaded = loadAcceptedLicenses()
    expect(loaded.a).toBe(true)
    expect(loaded.b).toBe(true)
  })

  it('loadAcceptedLicenses handles corrupt JSON gracefully', () => {
    localStorage.setItem('teletype.acceptedLicenses', 'not-json{')
    expect(loadAcceptedLicenses()).toEqual({})
  })

  it('loadAcceptedLicenses handles non-object JSON gracefully', () => {
    localStorage.setItem('teletype.acceptedLicenses', '"hello"')
    expect(loadAcceptedLicenses()).toEqual({})
  })

  it('getProgress after markDownloading with no progress is undefined', () => {
    markDownloading('only-active')
    expect(getProgress('only-active')).toBeUndefined()
  })

  it('notify via clearDownload triggers subscribers', () => {
    markDownloading('n1')
    const fn = vi.fn()
    subscribeDownloads(fn)
    clearDownload('n1')
    expect(fn).toHaveBeenCalled()
  })
})
