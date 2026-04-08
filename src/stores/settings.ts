import { defineStore } from 'pinia'
import { ref, computed, watchEffect } from 'vue'
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
    hotkey: appInfoStore.appInfo?.constants.default_hotkey ?? '',
    autostart: false,
    max_history: appInfoStore.appInfo?.constants.default_max_history ?? 0,
    theme: 'light',
    language: '',
    expiry_seconds: 0,
    capture_images: true,
    log_level: 'error',
  })
  const saving = ref(false)
  const saved = ref(false)

  // 预览覆盖层：只存设置页预览期间的临时值，不影响已保存的 settings
  const preview = ref<Partial<AppSettings>>({})

  /** '' 时跟随系统语言，优先读取 preview 覆盖层 */
  const effectiveLang = computed(() => {
    const lang = preview.value.language ?? settings.value.language
    if (lang === 'zh' || lang === 'en') return lang
    return navigator.language.startsWith('zh') ? 'zh' : 'en'
  })

  // 主题声明式自动应用，优先读取 preview 覆盖层
  watchEffect(() => {
    const theme = preview.value.theme ?? settings.value.theme
    document.documentElement.setAttribute('data-theme', theme)
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
        clearPreview()
        return
      }
      await saveSettings(patch)
      settings.value = await fetchSettings()
      clearPreview()
      saved.value = true
      setTimeout(() => (saved.value = false), 2000)
    } catch (e) {
      console.error('[settings] save failed:', e)
      throw e
    } finally {
      saving.value = false
    }
  }

  /** 设置预览覆盖，不持久化 */
  function setPreview(partial: Partial<AppSettings>) {
    preview.value = partial
  }

  /** 清除预览，恢复到已保存的展示状态 */
  function clearPreview() {
    preview.value = {}
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
    effectiveLang,
    load,
    save,
    setPreview,
    clearPreview,
    pauseHotkey,
    resumeHotkey,
  }
})
