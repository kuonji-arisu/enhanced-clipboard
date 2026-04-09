import { defineStore } from 'pinia'
import { computed, ref, watchEffect } from 'vue'
import {
  fetchSettings,
  pauseHotkey as pauseHotkeyApi,
  resumeHotkey as resumeHotkeyApi,
  saveSettings,
} from '../composables/settingsApi'
import { useAppInfoStore } from './appInfo'
import type { AppSettings, AppSettingsPatch, SaveSettingsResult } from '../types'

function buildDefaultSettings(appInfoStore: ReturnType<typeof useAppInfoStore>): AppSettings {
  return {
    hotkey: appInfoStore.appInfo?.default_hotkey ?? '',
    autostart: false,
    max_history: appInfoStore.appInfo?.default_max_history ?? 0,
    theme: 'light',
    expiry_seconds: 0,
    capture_images: true,
    log_level: 'error',
  }
}

function cloneSettings(settings: AppSettings): AppSettings {
  return { ...settings }
}

function replaceSettings(target: { value: AppSettings }, source: AppSettings) {
  target.value = cloneSettings(source)
}

function buildSettingsPatch(previous: AppSettings, next: AppSettings): AppSettingsPatch {
  const patch: AppSettingsPatch = {}
  for (const key of Object.keys(previous) as Array<keyof AppSettings>) {
    if (previous[key] !== next[key]) {
      patch[key] = next[key] as never
    }
  }
  return patch
}

export const useSettingsStore = defineStore('settings', () => {
  const appInfoStore = useAppInfoStore()
  const savedSettings = ref<AppSettings>(buildDefaultSettings(appInfoStore))
  const draftSettings = ref<AppSettings>(cloneSettings(savedSettings.value))
  const saving = ref(false)
  const saved = ref(false)
  const isDirty = computed(
    () => Object.keys(savedSettings.value).some((key) => {
      const settingsKey = key as keyof AppSettings
      return savedSettings.value[settingsKey] !== draftSettings.value[settingsKey]
    }),
  )

  watchEffect(() => {
    document.documentElement.setAttribute('data-theme', savedSettings.value.theme)
  })

  function markSaved() {
    saved.value = true
    setTimeout(() => {
      saved.value = false
    }, 2000)
  }

  async function load() {
    const settings = await fetchSettings()
    replaceSettings(savedSettings, settings)
    replaceSettings(draftSettings, settings)
  }

  function resetDraft() {
    replaceSettings(draftSettings, savedSettings.value)
  }

  async function save(): Promise<SaveSettingsResult> {
    const patch = buildSettingsPatch(savedSettings.value, draftSettings.value)
    if (Object.keys(patch).length === 0) {
      return {
        settings: cloneSettings(savedSettings.value),
        effects: {},
      }
    }

    saving.value = true
    saved.value = false
    try {
      const result = await saveSettings(patch)
      replaceSettings(savedSettings, result.settings)
      replaceSettings(draftSettings, result.settings)
      markSaved()
      return result
    } finally {
      saving.value = false
    }
  }

  async function pauseHotkey() {
    await pauseHotkeyApi()
  }

  async function resumeHotkey() {
    await resumeHotkeyApi()
  }

  return {
    savedSettings,
    draftSettings,
    isDirty,
    saving,
    saved,
    load,
    save,
    resetDraft,
    pauseHotkey,
    resumeHotkey,
  }
})
