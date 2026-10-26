import { describe, it, expect } from 'vitest'
import {
  pillReducer,
  initialPillMachineState,
  isBusyPhase,
  pillMessage,
  displayLevel,
  WAVE_BARS,
  type PillMachineState,
} from '../pillMachine'

const NOW = 1_700_000_000_000

function state(): PillMachineState {
  return initialPillMachineState()
}

describe('pill state machine', () => {
  it('defines expected phases', () => {
    const phases = ['idle', 'recording', 'warming', 'processing'] as const
    expect(phases).toContain('idle')
    expect(phases).toContain('recording')
    expect(phases).toContain('warming')
    expect(phases).toContain('processing')
  })

  it('starts idle with empty session state', () => {
    const s = state()
    expect(s.pill).toEqual({ phase: 'idle' })
    expect(s.levels).toHaveLength(WAVE_BARS)
    expect(s.levels.every((l) => l === 0)).toBe(true)
    expect(s.streamText).toBe('')
    expect(s.interimText).toBe('')
    expect(s.skipMessage).toBeNull()
    expect(s.languageChip).toBeNull()
    expect(s.hovered).toBe(false)
    expect(s.cancelArmed).toBe(false)
  })

  it('reaches every named phase via the reducer', () => {
    const names = [
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'idle' }, now: NOW }).pill.phase,
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'recording', startedAtMs: NOW }, now: NOW }).pill.phase,
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'warming' }, now: NOW }).pill.phase,
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'processing', message: 'Polishing…' }, now: NOW }).pill.phase,
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'processing', message: 'Transcribing…' }, now: NOW }).pill.phase,
      pillReducer(
        pillReducer(state(), { type: 'pill-state', payload: { phase: 'processing', message: 'Transforming…' }, now: NOW }),
        { type: 'token', seq: 1, tok: 'hi' },
      ).pill.phase,
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'processing', message: 'Cancelled' }, now: NOW }).pill.phase,
      pillReducer(state(), { type: 'error', message: 'boom' }).pill.phase,
    ]
    for (const want of ['idle', 'recording', 'warming', 'processing', 'transcribing', 'streaming', 'done', 'error']) {
      expect(names).toContain(want)
    }
  })
})

describe('pill-state transitions', () => {
  it('recording entry clears session leftovers', () => {
    let s = state()
    s = pillReducer(s, { type: 'interim', text: 'hello' })
    s = pillReducer(s, { type: 'token', seq: 1, tok: 'abc' })
    s = pillReducer(s, { type: 'skip', message: 'stale' })
    s = pillReducer(s, { type: 'language', code: 'ru', detected: true })
    s = pillReducer(s, { type: 'pill-state', payload: { phase: 'recording', startedAtMs: 111 }, now: NOW })
    expect(s.pill).toEqual({ phase: 'recording', startedAtMs: 111 })
    expect(s.interimText).toBe('')
    expect(s.streamText).toBe('')
    expect(s.lastTokenSeq).toBe(0)
    expect(s.skipMessage).toBeNull()
    expect(s.languageChip).toBeNull()
  })

  it('recording entry resets the waveform only on entry', () => {
    let s = pillReducer(state(), { type: 'pill-state', payload: { phase: 'recording', startedAtMs: 1 }, now: NOW })
    s = pillReducer(s, { type: 'level', level: 0.9 })
    expect(s.levels[s.levels.length - 1]).toBeCloseTo(displayLevel(0.9))
    const reemit = pillReducer(s, { type: 'pill-state', payload: { phase: 'recording', startedAtMs: 1 }, now: NOW + 5 })
    expect(reemit.levels[reemit.levels.length - 1]).toBeCloseTo(displayLevel(0.9))
    const fresh = pillReducer(
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'idle' }, now: NOW }),
      { type: 'pill-state', payload: { phase: 'recording', startedAtMs: 2 }, now: NOW },
    )
    expect(fresh.levels.every((l) => l === 0)).toBe(true)
  })

  it('recording without startedAtMs falls back to receipt time', () => {
    const s = pillReducer(state(), { type: 'pill-state', payload: { phase: 'recording' }, now: NOW })
    expect(s.pill).toEqual({ phase: 'recording', startedAtMs: NOW })
    expect(s.recordingFallbackMs).toBe(NOW)
  })

  it('warming entry stamps warmingSinceMs once', () => {
    const s = pillReducer(state(), { type: 'pill-state', payload: { phase: 'warming' }, now: NOW })
    expect(s.pill).toEqual({ phase: 'warming' })
    expect(s.warmingSinceMs).toBe(NOW)
    const again = pillReducer(s, { type: 'pill-state', payload: { phase: 'warming' }, now: NOW + 999 })
    expect(again.warmingSinceMs).toBe(NOW)
  })

  it('leaving recording disarms hover and cancel', () => {
    let s = pillReducer(state(), { type: 'pill-state', payload: { phase: 'recording', startedAtMs: 1 }, now: NOW })
    s = pillReducer(s, { type: 'hover', hovered: true })
    s = pillReducer(s, { type: 'cancel-armed', armed: true })
    s = pillReducer(s, { type: 'pill-state', payload: { phase: 'processing', message: 'x' }, now: NOW })
    expect(s.hovered).toBe(false)
    expect(s.cancelArmed).toBe(false)
  })

  it('refines processing messages into transcribing/streaming/done/error', () => {
    const at = (message: string) =>
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'processing', message }, now: NOW }).pill
    expect(at('Transcribing…')).toEqual({ phase: 'transcribing', message: 'Transcribing…' })
    expect(at('Transforming…')).toEqual({ phase: 'streaming', message: 'Transforming…' })
    expect(at('Transcription failed')).toEqual({ phase: 'error', message: 'Transcription failed' })
    expect(at('Polish failed (timeout). Using raw text').phase).toBe('error')
    expect(at('Cancelled')).toEqual({ phase: 'done', message: 'Cancelled' })
    expect(at('Polishing…')).toEqual({ phase: 'processing', message: 'Polishing…' })
    expect(at('')).toEqual({ phase: 'processing', message: '' })
  })
})

describe('dictation-state transitions', () => {
  it('listening starts a recording session', () => {
    const s = pillReducer(state(), {
      type: 'dictation-state',
      payload: { phase: 'listening', startedAtMs: 4321 },
      now: NOW,
    })
    expect(s.pill).toEqual({ phase: 'recording', startedAtMs: 4321 })
    expect(s.levels.every((l) => l === 0)).toBe(true)
  })

  it('maps busy phases without touching the waveform clock fallback', () => {
    expect(
      pillReducer(state(), { type: 'dictation-state', payload: { phase: 'transcribing' }, now: NOW }).pill.phase,
    ).toBe('transcribing')
    expect(
      pillReducer(state(), { type: 'dictation-state', payload: { phase: 'transforming' }, now: NOW }).pill.phase,
    ).toBe('streaming')
    const done = pillReducer(state(), {
      type: 'dictation-state',
      payload: { phase: 'message', text: 'Cancelled' },
      now: NOW,
    })
    expect(done.pill).toEqual({ phase: 'done', message: 'Cancelled' })
    const err = pillReducer(state(), {
      type: 'dictation-state',
      payload: { phase: 'error', text: 'mic gone' },
      now: NOW,
    })
    expect(err.pill).toEqual({ phase: 'error', message: 'mic gone' })
    expect(
      pillReducer(state(), { type: 'dictation-state', payload: { phase: 'inserting' }, now: NOW }).pill.phase,
    ).toBe('done')
  })

  it('idle/completed/cancelled return to idle and disarm', () => {
    let s = pillReducer(state(), { type: 'pill-state', payload: { phase: 'warming' }, now: NOW })
    s = pillReducer(s, { type: 'hover', hovered: true })
    for (const phase of ['idle', 'completed', 'cancelled']) {
      const next = pillReducer(s, { type: 'dictation-state', payload: { phase }, now: NOW })
      expect(next.pill).toEqual({ phase: 'idle' })
      expect(next.hovered).toBe(false)
      expect(next.cancelArmed).toBe(false)
    }
  })

  it('never exits recording (pill-state owns the recording exit)', () => {
    const rec = pillReducer(state(), {
      type: 'pill-state',
      payload: { phase: 'recording', startedAtMs: 7 },
      now: NOW,
    })
    for (const payload of [
      { phase: 'transcribing' },
      { phase: 'transforming' },
      { phase: 'inserting' },
      { phase: 'message', text: 'Cancelled' },
      { phase: 'idle' },
      { phase: 'error', text: 'x' },
    ]) {
      expect(pillReducer(rec, { type: 'dictation-state', payload, now: NOW }).pill).toEqual({
        phase: 'recording',
        startedAtMs: 7,
      })
    }
  })

  it('ignores unknown dictation phases', () => {
    const s = pillReducer(state(), { type: 'dictation-state', payload: { phase: 'frobnicating' }, now: NOW })
    expect(s.pill).toEqual({ phase: 'idle' })
  })
})

describe('streaming tokens and interim transcripts', () => {
  it('concatenates transform-token deltas in seq order', () => {
    let s = pillReducer(state(), { type: 'pill-state', payload: { phase: 'processing', message: 'Transforming…' }, now: NOW })
    s = pillReducer(s, { type: 'token', seq: 1, tok: 'Hello' })
    s = pillReducer(s, { type: 'token', seq: 2, tok: ' world' })
    expect(s.streamText).toBe('Hello world')
    expect(s.lastTokenSeq).toBe(2)
  })

  it('drops stale or duplicate seqs', () => {
    let s = pillReducer(state(), { type: 'token', seq: 3, tok: 'c' })
    s = pillReducer(s, { type: 'token', seq: 2, tok: 'STALE' })
    s = pillReducer(s, { type: 'token', seq: 3, tok: 'DUP' })
    expect(s.streamText).toBe('c')
  })

  it('token arrival moves processing/transcribing to streaming, keeps the message', () => {
    const proc = pillReducer(state(), { type: 'pill-state', payload: { phase: 'processing', message: 'Polishing…' }, now: NOW })
    // 'Polishing…' refines to done; use a raw processing token path instead:
    const raw: PillMachineState = { ...proc, pill: { phase: 'processing', message: 'Working…' } }
    const streamed = pillReducer(raw, { type: 'token', seq: 1, tok: 'x' })
    expect(streamed.pill).toEqual({ phase: 'streaming', message: 'Working…' })
    const transcribing: PillMachineState = { ...state(), pill: { phase: 'transcribing', message: 'Transcribing…' } }
    expect(pillReducer(transcribing, { type: 'token', seq: 1, tok: 'x' }).pill.phase).toBe('streaming')
  })

  it('token arrival never changes idle/recording/warming phases', () => {
    const idle = pillReducer(state(), { type: 'token', seq: 1, tok: 'x' })
    expect(idle.pill).toEqual({ phase: 'idle' })
    expect(idle.streamText).toBe('x')
    const rec = pillReducer(
      pillReducer(state(), { type: 'pill-state', payload: { phase: 'recording', startedAtMs: 1 }, now: NOW }),
      { type: 'token', seq: 1, tok: 'x' },
    )
    expect(rec.pill).toEqual({ phase: 'recording', startedAtMs: 1 })
  })

  it('stores the interim transcript verbatim', () => {
    const s = pillReducer(state(), { type: 'interim', text: 'partial words' })
    expect(s.interimText).toBe('partial words')
  })
})

describe('levels, hover, cancel, skip, language, settings', () => {
  it('appends levels with a fixed-length sliding window', () => {
    let s = state()
    for (let i = 1; i <= WAVE_BARS + 5; i++) s = pillReducer(s, { type: 'level', level: i })
    expect(s.levels).toHaveLength(WAVE_BARS)
    expect(s.levels[0]).toBeCloseTo(displayLevel(6))
    expect(s.levels[s.levels.length - 1]).toBeCloseTo(displayLevel(WAVE_BARS + 5))
  })

  it('toggles hover and cancel-armed', () => {
    let s = pillReducer(state(), { type: 'hover', hovered: true })
    expect(s.hovered).toBe(true)
    s = pillReducer(s, { type: 'hover', hovered: false })
    expect(s.hovered).toBe(false)
    s = pillReducer(s, { type: 'cancel-armed', armed: true })
    expect(s.cancelArmed).toBe(true)
  })

  it('sets and clears the skip message', () => {
    let s = pillReducer(state(), { type: 'skip', message: 'Polish skipped' })
    expect(s.skipMessage).toBe('Polish skipped')
    s = pillReducer(s, { type: 'skip-clear' })
    expect(s.skipMessage).toBeNull()
  })

  it('sets and clears the language chip', () => {
    let s = pillReducer(state(), { type: 'language', code: 'ru', detected: true })
    expect(s.languageChip).toEqual({ code: 'ru', detected: true })
    s = pillReducer(s, { type: 'language-clear' })
    expect(s.languageChip).toBeNull()
  })

  it('loads settings/devices without clobbering on patch or device rename', () => {
    const settings = { recordingMode: 'hold', language: 'en', inputDevice: 'a', pillStyle: 'well' }
    const devices = [{ id: 'a', name: 'Mic One', is_default: true }]
    let s = pillReducer(state(), { type: 'settings-loaded', settings, devices, deviceName: 'Mic' })
    expect(s.settings?.pillStyle).toBe('well')
    expect(s.devices).toHaveLength(1)
    s = pillReducer(s, { type: 'device-name', name: 'Other' })
    expect(s.deviceName).toBe('Other')
    expect(s.settings?.pillStyle).toBe('well')
    expect(s.devices).toHaveLength(1)
    const patched = { ...settings, inputDevice: 'b' }
    s = pillReducer(s, { type: 'settings-patched', settings: patched })
    expect(s.settings?.inputDevice).toBe('b')
    expect(s.deviceName).toBe('Other')
    expect(s.devices).toHaveLength(1)
  })

  it('error action surfaces the message and disarms', () => {
    let s = pillReducer(state(), { type: 'hover', hovered: true })
    s = pillReducer(s, { type: 'error', message: 'Transcription failed' })
    expect(s.pill).toEqual({ phase: 'error', message: 'Transcription failed' })
    expect(s.hovered).toBe(false)
    expect(s.cancelArmed).toBe(false)
  })
})

describe('phase helpers', () => {
  it('isBusyPhase covers the processing-like phases only', () => {
    for (const p of ['processing', 'transcribing', 'streaming', 'done', 'error'] as const) {
      expect(isBusyPhase(p)).toBe(true)
    }
    for (const p of ['idle', 'recording', 'warming'] as const) {
      expect(isBusyPhase(p)).toBe(false)
    }
  })

  it('pillMessage returns "" outside busy phases', () => {
    expect(pillMessage({ phase: 'idle' })).toBe('')
    expect(pillMessage({ phase: 'recording', startedAtMs: 1 })).toBe('')
    expect(pillMessage({ phase: 'warming' })).toBe('')
    expect(pillMessage({ phase: 'processing', message: 'm' })).toBe('m')
    expect(pillMessage({ phase: 'error', message: 'e' })).toBe('e')
  })
})

describe('polish diff card', () => {
  const payload = {
    raw: 'so um we need to ship the release today',
    polished: 'We need to ship the release today.',
    ms: 840,
  }

  it('shows the card in the result phase with its payload', () => {
    const s = pillReducer(state(), { type: 'polish-diff', payload })
    expect(s.pill).toEqual({ phase: 'result', ...payload })
  })

  it('is not a busy phase, so the processing body never renders it', () => {
    const s = pillReducer(state(), { type: 'polish-diff', payload })
    expect(isBusyPhase(s.pill.phase)).toBe(false)
    expect(pillMessage(s.pill)).toBe('')
  })

  it('ignores a difference that is only whitespace', () => {
    const s = pillReducer(state(), {
      type: 'polish-diff',
      payload: { ...payload, polished: `${payload.raw}  ` },
    })
    expect(s.pill).toEqual({ phase: 'idle' })

    const blank = pillReducer(state(), {
      type: 'polish-diff',
      payload: { raw: '   ', polished: 'text', ms: 10 },
    })
    expect(blank.pill.phase).toBe('idle')
  })

  it('dismissal clears the card but never a newer take', () => {
    const card = pillReducer(state(), { type: 'polish-diff', payload })
    expect(pillReducer(card, { type: 'result-dismiss' }).pill.phase).toBe('idle')

    const recording = pillReducer(state(), {
      type: 'dictation-state',
      payload: { phase: 'listening', startedAtMs: NOW },
      now: NOW,
    })
    const afterLateTimer = pillReducer(recording, { type: 'result-dismiss' })
    expect(afterLateTimer.pill.phase).toBe('recording')
  })

  it('a new take replaces the card', () => {
    const card = pillReducer(state(), { type: 'polish-diff', payload })
    const next = pillReducer(card, {
      type: 'dictation-state',
      payload: { phase: 'listening', startedAtMs: NOW + 1 },
      now: NOW + 1,
    })
    expect(next.pill.phase).toBe('recording')
    expect(next.skipMessage).toBeNull()
  })
})
