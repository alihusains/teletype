// claim: ui-behavior
import { describe, it, expect } from 'vitest'
import {
  GLOW_BARS,
  GLOW_BAR_MAX_PX,
  GLOW_BAR_MIN_PX,
  barScale,
  glowFrameTitle,
  glowSentence,
  glowTone,
  glowView,
  isWorkingMessage,
  knownPillStyle,
  pushSample,
  stripEllipsis,
} from '../lib/glowModel'
import { initialPillMachineState, type PillMachineState } from '../pillMachine'

describe('isWorkingMessage', () => {
  it('treats progress labels as work in progress', () => {
    expect(isWorkingMessage('transcribing', 'Transcribing…')).toBe(true)
    expect(isWorkingMessage('streaming', 'Transforming…')).toBe(true)
    expect(isWorkingMessage('done', 'Inserting…')).toBe(true)
    expect(isWorkingMessage('processing', 'Polishing...')).toBe(true)
  })

  it('treats finished notes and errors as notes', () => {
    expect(isWorkingMessage('processing', 'Pasted')).toBe(false)
    expect(isWorkingMessage('processing', 'Too short')).toBe(false)
    expect(isWorkingMessage('done', 'Cancelled')).toBe(false)
    expect(isWorkingMessage('error', 'Transcription failed…')).toBe(false)
    expect(isWorkingMessage('error', "Couldn't load the speech model (missing file)")).toBe(false)
  })
})

describe('glowTone', () => {
  it('maps every note to its tone', () => {
    expect(glowTone('processing', 'Pasted', null)).toBe('success')
    expect(glowTone('processing', 'Too short', null)).toBe('warn')
    expect(glowTone('done', 'Cancelled', null)).toBe('neutral')
    expect(glowTone('error', 'Pasted', null)).toBe('error')
    expect(glowTone('streaming', 'Transforming…', 'Kept your words as-is')).toBe('warn')
  })
})

describe('barScale', () => {
  const floor = GLOW_BAR_MIN_PX / GLOW_BAR_MAX_PX

  it('maps silence to the 4px floor and full scale to the full bar', () => {
    expect(barScale(0)).toBeCloseTo(4 / 22)
    expect(barScale(1)).toBe(1)
  })

  it('clamps out-of-range and non-finite input', () => {
    expect(barScale(Number.NaN)).toBeCloseTo(floor)
    expect(barScale(-3)).toBeCloseTo(floor)
    expect(barScale(Number.POSITIVE_INFINITY)).toBeCloseTo(floor)
    expect(barScale(2)).toBe(1)
  })
})

describe('pushSample', () => {
  it('keeps the window length, puts the newest sample on the right, and copies', () => {
    const history = new Array(GLOW_BARS).fill(0)
    const frozen = Object.freeze([...history])
    const next = pushSample(frozen, 0.7)
    expect(next).toHaveLength(11)
    expect(next[10]).toBe(0.7)
    expect(next.slice(0, 10).every((v) => v === 0)).toBe(true)
    expect(frozen[10]).toBe(0)
    expect(next).not.toBe(frozen)
    const again = pushSample(next, 0.2)
    expect(again[9]).toBe(0.7)
    expect(again[10]).toBe(0.2)
  })
})

function machineIn(pill: PillMachineState['pill'], extra: Partial<PillMachineState> = {}): PillMachineState {
  return { ...initialPillMachineState(), pill, ...extra }
}

describe('glowView', () => {
  it('maps machine phases onto the four glow views', () => {
    expect(glowView(machineIn({ phase: 'idle' }))).toBeNull()
    expect(glowView(machineIn({ phase: 'result', raw: 'a', polished: 'b', ms: 1 }))).toBeNull()
    expect(glowView(machineIn({ phase: 'warming' }))).toBe('warming')
    expect(glowView(machineIn({ phase: 'recording', startedAtMs: 1 }))).toBe('recording')
    expect(glowView(machineIn({ phase: 'transcribing', message: 'Transcribing…' }))).toBe('working')
    expect(glowView(machineIn({ phase: 'done', message: 'Inserting…' }))).toBe('working')
    expect(glowView(machineIn({ phase: 'processing', message: 'Pasted' }))).toBe('note')
    expect(glowView(machineIn({ phase: 'done', message: 'Cancelled' }))).toBe('note')
    expect(glowView(machineIn({ phase: 'error', message: 'Paste failed' }))).toBe('note')
  })

  it('shows a note whenever a skip line or the mic action needs room', () => {
    expect(
      glowView(machineIn({ phase: 'streaming', message: 'Transforming…' }, { skipMessage: 'Kept your words' })),
    ).toBe('note')
    expect(
      glowView(machineIn({ phase: 'transcribing', message: 'Transcribing…' }, { micFailure: true })),
    ).toBe('note')
  })
})

describe('glowSentence', () => {
  it('names every phase in words', () => {
    expect(glowSentence(machineIn({ phase: 'recording', startedAtMs: 1 }))).toBe('Recording')
    expect(glowSentence(machineIn({ phase: 'recording', startedAtMs: 1, command: true }))).toBe('Recording a command')
    expect(glowSentence(machineIn({ phase: 'warming' }))).toBe('Getting model ready')
    expect(glowSentence(machineIn({ phase: 'transcribing', message: 'Transcribing…' }))).toBe('Transcribing')
    expect(glowSentence(machineIn({ phase: 'streaming', message: 'Transforming…' }))).toBe('Transforming')
    expect(glowSentence(machineIn({ phase: 'done', message: 'Inserting…' }))).toBe('Inserting')
    expect(glowSentence(machineIn({ phase: 'processing', message: 'Pasted' }))).toBe('Pasted')
  })

  it('announces download progress only at 25% steps', () => {
    expect(glowSentence(machineIn({ phase: 'warming' }, { loadProgress: 0.1 }))).toBe('Downloading model 0%')
    expect(glowSentence(machineIn({ phase: 'warming' }, { loadProgress: 0.42 }))).toBe('Downloading model 25%')
    expect(glowSentence(machineIn({ phase: 'warming' }, { loadProgress: 0.99 }))).toBe('Downloading model 75%')
    expect(glowSentence(machineIn({ phase: 'warming' }, { loadProgress: 1 }))).toBe('Downloading model 100%')
  })

  it('speaks a note\'s skip line after its message', () => {
    const skip = 'Polish skipped: no model loaded. Using raw text'
    expect(glowSentence(machineIn({ phase: 'done', message: 'Inserting…' }, { skipMessage: skip }))).toBe(
      `Inserting. ${skip}`,
    )
    expect(glowSentence(machineIn({ phase: 'processing', message: 'Pasted' }, { skipMessage: skip }))).toBe(
      `Pasted. ${skip}`,
    )
  })

  it('strips a trailing ellipsis in either spelling', () => {
    expect(stripEllipsis('Polishing...')).toBe('Polishing')
    expect(stripEllipsis('Inserting… ')).toBe('Inserting')
    expect(stripEllipsis('Pasted')).toBe('Pasted')
  })
})

describe('knownPillStyle', () => {
  it('passes the six known ids through and falls back to default', () => {
    for (const id of ['default', 'classic', 'levelRail', 'well', 'dotGrid', 'glow']) {
      expect(knownPillStyle(id)).toBe(id)
    }
    expect(knownPillStyle('futureStyle')).toBe('default')
    expect(knownPillStyle('unknown')).toBe('default')
    expect(knownPillStyle('')).toBe('default')
    expect(knownPillStyle(undefined)).toBe('classic')
  })
})

describe('glowFrameTitle', () => {
  it('names the keyboard path for every view', () => {
    expect(glowFrameTitle('warming', 'warming', 'Getting model ready')).toBe('Getting model ready. Press Esc to cancel.')
    expect(glowFrameTitle('recording', 'recording', 'Recording')).toBe(
      'Recording. Press your hotkey to stop, or Esc to cancel.',
    )
    expect(glowFrameTitle('working', 'transcribing', 'Transcribing')).toBe('Transcribing. Press Esc to cancel.')
    expect(glowFrameTitle('working', 'streaming', 'Transforming')).toBe('Transforming. Press Esc to cancel.')
    // Inserting is past the point a cancel can stop.
    expect(glowFrameTitle('working', 'done', 'Inserting')).toBe('Inserting.')
    expect(glowFrameTitle('note', 'processing', 'Pasted')).toBe('Press your hotkey to dictate again.')
  })
})
