import { defineStore } from 'pinia'
import { ref, watchEffect } from 'vue'
import {
  fetchSettings,
  pauseHotkey as pauseHotkeyApi,
  resumeHotkey as resumeHotkeyApi,
  saveSettings,
} from '../composables/settingsApi'
import { useAppInfoStore } from './appInfo'
import type { AppSettings, AppSettingsPatch } from '../types'

function buildSettingsPatch(
  previous: AppSettings,
  next: Partial<AppSettings>,
): AppSettingsPatch {
  const patch: AppSettingsPatch = {}
  for (const key of Object.keys(previous) as Array<keyof AppSettings>) {
    const value = next[key]
    if (value !== undefined && previous[key] !== value) {
      patch[key] = value as never
    }
  }
  return patch
}

export const useSettingsStore = defineStore('settings', () => {
  const appInfoStore = useAppInfoStore()
  const settings = ref<AppSettings>({
    hotkey: appInfoStore.appInfo?.default_hotkey ?? '',
    autostart: false,
    max_history: appInfoStore.appInfo?.default_max_history ?? 0,
    theme: 'light',
    expiry_seconds: 0,
    capture_images: true,
    log_level: 'error',
  })
  const saving = ref(false)
  const saved = ref(false)

  // 主题声明式自动应用，仅跟随已保存设置
  watchEffect(() => {
    document.documentElement.setAttribute('data-theme', settings.value.theme)
  })

  async function load() {
    settings.value = await fetchSettings()
  }

  /** 保存 draft 到后端；成功后 settings 更新为已保存值，并清除预览 */
  async function save(draft: AppSettingsPatch) {
    saving.value = true
    saved.value = false
    try {
      const patch = buildSettingsPatch(settings.value, draft)
      if (Object.keys(patch).length === 0) {
        return
      }
      await saveSettings(patch)
      settings.value = await fetchSettings()
      saved.value = true
      setTimeout(() => (saved.value = false), 2000)
    } catch (e) {
      console.error('[settings] save failed:', e)
      throw e
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
    settings,
    saving,
    saved,
    load,
    save,
    pauseHotkey,
    resumeHotkey,
  }
})
