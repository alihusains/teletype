import { describe, it, expect } from 'vitest'
import type { Settings } from '../screens/SettingsScreen'

const REQUIRED_SETTINGS_FIELDS: (keyof Settings)[] = [
  'hotkey', 'recordingMode', 'selectedSpeechModel', 'language', 'inputDevice',
  'restoreClipboard', 'keepTextOnClipboard', 'autoApplyTransform', 'showTrayIcon',
  'hasCompletedOnboarding', 'selectedLlmModel', 'typingAutotextEnabled', 'autotextTiming',
  'removeFillerWords', 'fillerWords', 'pillPosition', 'alwaysShowPill', 'livePreviewEnabled',
  'cancelHotkey', 'smartInsertion', 'playRecordingSounds', 'recordingSound', 'warmEnginePolicy',
  'matchStrictness', 'dictionaryEnabled', 'pillStyle', 'appIcon', 'transcriptsDir',
  'vadAutoStop', 'vadSilenceMs', 'enableDeveloperTab', 'polishGateEnabled',
  'polishGateThresholdWords', 'restoreEmoji', 'spokenEmoji', 'spokenPunctuation',
  'modelUnloadDelaySecs', 'activeStyleProfile', 'appLanguageOverrides', 'enabledPacks',
  'selectedLlmProvider', 'theme', 'reduceMotion', 'polishRules', 'listStyle', 'quickAddHotkey',
]

describe('Settings shape', () => {
  it('has exactly 46 fields', () => {
    expect(REQUIRED_SETTINGS_FIELDS.length).toBe(46)
  })

  it('all fields are unique', () => {
    expect(new Set(REQUIRED_SETTINGS_FIELDS).size).toBe(46)
  })

  it('fixture with all 46 fields serializes correctly', () => {
    const fixture = Object.fromEntries(REQUIRED_SETTINGS_FIELDS.map((k) => [k, k.includes('Enabled') || k.startsWith('restore') || k.startsWith('keep') || k.startsWith('show') || k.startsWith('has') || k === 'removeFillerWords' || k === 'alwaysShowPill' || k === 'livePreviewEnabled' || k === 'smartInsertion' || k === 'playRecordingSounds' || k === 'dictionaryEnabled' || k === 'vadAutoStop' || k === 'enableDeveloperTab' || k === 'polishGateEnabled' || k === 'spokenEmoji' || k === 'restoreEmoji' || k === 'spokenPunctuation' || k === 'reduceMotion' ? false : typeof k === 'string' && (k.endsWith('Ms') || k.endsWith('Secs') || k === 'polishGateThresholdWords' ? 0 : Array.isArray([]) ? [] : typeof k)])) as Record<string, unknown>
    expect(Object.keys(fixture).length).toBe(46)
    const json = JSON.stringify(fixture)
    expect(JSON.parse(json)).toEqual(fixture)
  })

  it('roundtrips booleans, numbers, strings, arrays, and records via JSON', () => {
    const sample: Settings = {
      hotkey: 'Alt+Space',
      recordingMode: 'hold',
      selectedSpeechModel: 'whisper-large',
      language: 'en',
      inputDevice: 'default',
      restoreClipboard: true,
      keepTextOnClipboard: false,
      autoApplyTransform: false,
      showTrayIcon: true,
      hasCompletedOnboarding: true,
      selectedLlmModel: 'llama-3',
      typingAutotextEnabled: true,
      autotextTiming: 'immediate',
      removeFillerWords: true,
      fillerWords: ['um', 'uh'],
      pillPosition: 'topRight',
      alwaysShowPill: true,
      livePreviewEnabled: false,
      cancelHotkey: 'Escape',
      smartInsertion: true,
      playRecordingSounds: true,
      recordingSound: 'Pop',
      warmEnginePolicy: 'always',
      matchStrictness: 'strict',
      dictionaryEnabled: true,
      pillStyle: 'compact',
      appIcon: 'white',
      transcriptsDir: '/tmp/transcripts',
      vadAutoStop: true,
      vadSilenceMs: 1200,
      enableDeveloperTab: false,
      polishGateEnabled: true,
      polishGateThresholdWords: 20,
      restoreEmoji: true,
      spokenEmoji: false,
      spokenPunctuation: true,
      modelUnloadDelaySecs: 30,
      activeStyleProfile: 'formal',
      appLanguageOverrides: { fr: 'en' },
      enabledPacks: ['pack-a'],
      selectedLlmProvider: 'local',
      theme: 'dark',
      reduceMotion: false,
      polishRules: { ruleA: true },
      listStyle: 'numbered',
      quickAddHotkey: 'Cmd+Shift+Q',
    }
    const parsed = JSON.parse(JSON.stringify(sample)) as Settings
    expect(parsed).toEqual(sample)
    expect(parsed.fillerWords).toEqual(['um', 'uh'])
    expect(parsed.appLanguageOverrides).toEqual({ fr: 'en' })
  })

  it('default-like sample has expected types per field', () => {
    const s: Settings = {
      hotkey: '', recordingMode: '', selectedSpeechModel: '', language: '', inputDevice: '',
      restoreClipboard: false, keepTextOnClipboard: false, autoApplyTransform: false, showTrayIcon: false,
      hasCompletedOnboarding: false, selectedLlmModel: '', typingAutotextEnabled: false, autotextTiming: '',
      removeFillerWords: false, fillerWords: [], pillPosition: '', alwaysShowPill: false, livePreviewEnabled: false,
      cancelHotkey: '', smartInsertion: false, playRecordingSounds: false, recordingSound: '', warmEnginePolicy: '',
      matchStrictness: '', dictionaryEnabled: false, pillStyle: '', appIcon: '', transcriptsDir: '',
      vadAutoStop: false, vadSilenceMs: 0, enableDeveloperTab: false, polishGateEnabled: false,
      polishGateThresholdWords: 0, restoreEmoji: false, spokenEmoji: false, spokenPunctuation: false,
      modelUnloadDelaySecs: 0, activeStyleProfile: '', appLanguageOverrides: {}, enabledPacks: [],
      selectedLlmProvider: '', theme: '', reduceMotion: false, polishRules: {}, listStyle: '', quickAddHotkey: '',
    }
    expect(typeof s.vadSilenceMs).toBe('number')
    expect(typeof s.hotkey).toBe('string')
    expect(Array.isArray(s.fillerWords)).toBe(true)
    expect(typeof s.appLanguageOverrides).toBe('object')
  })
})
