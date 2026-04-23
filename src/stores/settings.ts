import { defineStore } from 'pinia'
import { computed, ref, watchEffect } from 'vue'
import {
  fetchSettings,
  pauseHotkey as pauseHotkeyApi,
  resumeHotkey as resumeHotkeyApi,
  saveSettings,
} from '../composables/settingsApi'
import { useAppInfoStore } from './appInfo'
import { useRuntimeStore } from './runtime'
import {
  beginSettingsSaveVisibilitySession,
  cancelSettingsSaveVisibilitySession,
  finishSettingsSaveVisibilitySession,
} from '../utils/clipboardViewCoordinator'
import type {
  AppSettings,
  AppSettingsPatch,
  EffectiveTheme,
  SaveSettingsResult,
  ThemeMode,
} from '../types'

function buildDefaultSettings(appInfoStore: ReturnType<typeof useAppInfoStore>): AppSettings {
  return {
    hotkey: appInfoStore.appInfo?.default_hotkey ?? '',
    autostart: false,
    max_history: appInfoStore.appInfo?.default_max_history ?? 0,
    theme_mode: 'light',
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

function resolveEffectiveTheme(themeMode: ThemeMode, systemTheme: EffectiveTheme): EffectiveTheme {
  return themeMode === 'system' ? systemTheme : themeMode
}

export const useSettingsStore = defineStore('settings', () => {
  const appInfoStore = useAppInfoStore()
  const runtimeStore = useRuntimeStore()
  const savedSettings = ref<AppSettings>(buildDefaultSettings(appInfoStore))
  const draftSettings = ref<AppSettings>(cloneSettings(savedSettings.value))
  const previewThemeMode = ref<ThemeMode | null>(null)
  const saving = ref(false)
  const saved = ref(false)
  const effectiveTheme = computed<EffectiveTheme>(() =>
    resolveEffectiveTheme(
      previewThemeMode.value ?? savedSettings.value.theme_mode,
      runtimeStore.runtime.system_theme,
    ),
  )
  const isDirty = computed(
    () => Object.keys(savedSettings.value).some((key) => {
      const settingsKey = key as keyof AppSettings
      return savedSettings.value[settingsKey] !== draftSettings.value[settingsKey]
    }),
  )

  watchEffect(() => {
    document.documentElement.setAttribute('data-theme', effectiveTheme.value)
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

  function setThemePreview(themeMode: ThemeMode) {
    previewThemeMode.value = themeMode
  }

  function clearThemePreview() {
    previewThemeMode.value = null
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
    beginSettingsSaveVisibilitySession()
    try {
      const result = await saveSettings(patch)
      replaceSettings(savedSettings, result.settings)
      replaceSettings(draftSettings, result.settings)
      await finishSettingsSaveVisibilitySession()
      markSaved()
      return result
    } catch (error) {
      cancelSettingsSaveVisibilitySession()
      throw error
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
    effectiveTheme,
    isDirty,
    saving,
    saved,
    load,
    save,
    resetDraft,
    setThemePreview,
    clearThemePreview,
    pauseHotkey,
    resumeHotkey,
  }
})
