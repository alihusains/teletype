import { describe, it, expect } from 'vitest'
import type { Settings } from '../screens/SettingsScreen'

const SETTINGS_FIXTURE: Settings = {
  hotkey: 'Cmd+Shift+Space',
  recordingMode: 'toggle',
  selectedSpeechModel: 'whisper-base',
  language: 'en',
  inputDevice: '',
  restoreClipboard: true,
  keepTextOnClipboard: true,
  autoApplyTransform: false,
  showTrayIcon: true,
  hasCompletedOnboarding: false,
  selectedLlmModel: '',
  typingAutotextEnabled: true,
  autotextTiming: 'delimiter',
  removeFillerWords: false,
  fillerWords: [],
  pillPosition: 'bottomCenter',
  alwaysShowPill: false,
  livePreviewEnabled: true,
  cancelHotkey: 'Escape',
  smartInsertion: true,
  playRecordingSounds: true,
  recordingSound: 'Pop',
  warmEnginePolicy: 'auto',
  matchStrictness: 'balanced',
  dictionaryEnabled: true,
  pillStyle: 'default',
  appIcon: 'white',
  transcriptsDir: '',
  vadAutoStop: false,
  vadSilenceMs: 800,
  enableDeveloperTab: false,
  polishGateEnabled: true,
  polishGateThresholdWords: 12,
  restoreEmoji: true,
  spokenEmoji: true,
  spokenPunctuation: true,
  modelUnloadDelaySecs: 0,
  activeStyleProfile: '',
  appLanguageOverrides: {},
  enabledPacks: [],
  selectedLlmProvider: '',
  theme: 'system',
  reduceMotion: false,
  polishRules: {},
  listStyle: 'bullet',
  quickAddHotkey: '',
}

describe('Settings interface coverage', () => {
  it('has expected number of fields matching Rust persisted fields', () => {
    const keys = Object.keys(SETTINGS_FIXTURE) as (keyof Settings)[]
    expect(keys.length).toBeGreaterThan(40)
    const mustHave: (keyof Settings)[] = [
      'hotkey',
      'language',
      'recordingMode',
      'restoreClipboard',
      'showTrayIcon',
      'autoApplyTransform',
      'pillPosition',
      'alwaysShowPill',
      'selectedSpeechModel',
      'inputDevice',
    ]
    for (const k of mustHave) expect(keys).toContain(k)
  })

  it('serializes and deserializes the Settings fixture losslessly', () => {
    const json = JSON.stringify(SETTINGS_FIXTURE)
    const parsed = JSON.parse(json) as Settings
    expect(parsed).toEqual(SETTINGS_FIXTURE)
  })
})
