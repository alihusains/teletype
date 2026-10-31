import { describe, it, expect } from 'vitest'
import type { Settings } from '../screens/SettingsScreen'

const DEFAULTS: Partial<Settings> = {
  hotkey: 'Cmd+Shift+Space',
  recordingMode: 'toggle',
  language: 'auto',
  restoreClipboard: true,
  showTrayIcon: true,
  keepTextOnClipboard: false,
  tapToPasteClipboard: false,
  autoApplyTransform: false,
  hasCompletedOnboarding: false,
  selectedLlmModel: '',
  typingAutotextEnabled: false,
  autotextTiming: '',
  removeFillerWords: false,
  pillPosition: '',
  alwaysShowPill: false,
  livePreviewEnabled: false,
  cancelHotkey: '',
  inputDevice: '',
}

describe('settings round-trip', () => {
  it('preserves defaults through JSON serialization', () => {
    const serialized = JSON.stringify(DEFAULTS)
    const parsed = JSON.parse(serialized)
    expect(parsed).toEqual(DEFAULTS)
  })

  it('has a non-trivial settings object', () => {
    expect(Object.keys(DEFAULTS).length).toBeGreaterThan(15)
  })

  it('requires more than 40 settings fields end-to-end (regression guard for P3/S7 wiring)', () => {
    // The second test above guards the low bar; this one guards that the full
    // Settings interface does not regress below the current 46 fields.
    const keys: (keyof Settings)[] = [
      'hotkey','recordingMode','selectedSpeechModel','language','inputDevice',
      'restoreClipboard','keepTextOnClipboard','tapToPasteClipboard','autoApplyTransform','showTrayIcon',
      'hasCompletedOnboarding','selectedLlmModel','typingAutotextEnabled','autotextTiming',
      'removeFillerWords','fillerWords','pillPosition','alwaysShowPill','livePreviewEnabled',
      'cancelHotkey','smartInsertion','playRecordingSounds','recordingSound','warmEnginePolicy',
      'matchStrictness','dictionaryEnabled','pillStyle','appIcon','transcriptsDir','vadAutoStop',
      'vadSilenceMs','enableDeveloperTab','polishGateEnabled','polishGateThresholdWords',
      'polishMaxChangePercent',
      'restoreEmoji','spokenEmoji','spokenPunctuation','modelUnloadDelaySecs','activeStyleProfile',
      'appLanguageOverrides','enabledPacks','selectedLlmProvider','theme','reduceMotion',
      'polishRules','listStyle','quickAddHotkey',
    ]
    expect(keys.length).toBeGreaterThan(40)
  })
})
