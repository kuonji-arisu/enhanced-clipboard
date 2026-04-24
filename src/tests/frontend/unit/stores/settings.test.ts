import { nextTick } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useSettingsStore } from '../../../../stores/settings'
import {
  createAppInfo,
  createAppSettings,
  createRuntimeStatus,
} from '../../support/factories'
import {
  installTestPinia,
  primeAppInfoStore,
  primeRuntimeStore,
} from '../../support/pinia'
import { setTauriInvokeHandler } from '../../support/tauri'

const coordinatorMocks = vi.hoisted(() => ({
  beginSettingsSaveVisibilitySession: vi.fn(),
  cancelSettingsSaveVisibilitySession: vi.fn(),
  finishSettingsSaveVisibilitySession: vi.fn(async () => undefined),
}))

vi.mock('../../../../utils/clipboardViewCoordinator', () => ({
  beginSettingsSaveVisibilitySession: coordinatorMocks.beginSettingsSaveVisibilitySession,
  cancelSettingsSaveVisibilitySession: coordinatorMocks.cancelSettingsSaveVisibilitySession,
  finishSettingsSaveVisibilitySession: coordinatorMocks.finishSettingsSaveVisibilitySession,
}))

describe('settings store', () => {
  beforeEach(() => {
    installTestPinia()
    primeAppInfoStore(createAppInfo())
    primeRuntimeStore(createRuntimeStatus({ system_theme: 'dark' }))
  })

  it('loads settings, resolves effective theme, and persists only changed fields', async () => {
    vi.useFakeTimers()

    const initialSettings = createAppSettings({
      theme_mode: 'system',
      max_history: 500,
    })
    const savedSettings = createAppSettings({
      theme_mode: 'dark',
      max_history: 750,
    })
    let savedPatch: Record<string, unknown> | null = null

    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_settings') {
        return initialSettings
      }
      if (command === 'save_settings') {
        savedPatch = args?.patch as Record<string, unknown>
        return {
          settings: savedSettings,
          effects: {},
        }
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const store = useSettingsStore()
    await store.load()
    store.setThemePreview('system')
    await nextTick()

    expect(store.effectiveTheme).toBe('dark')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')

    store.clearThemePreview()
    store.draftSettings = {
      ...store.draftSettings,
      max_history: 750,
      theme_mode: 'dark',
    }
    const result = await store.save()

    expect(savedPatch).toEqual({
      max_history: 750,
      theme_mode: 'dark',
    })
    expect(coordinatorMocks.beginSettingsSaveVisibilitySession).toHaveBeenCalledOnce()
    expect(coordinatorMocks.finishSettingsSaveVisibilitySession).toHaveBeenCalledOnce()
    expect(coordinatorMocks.cancelSettingsSaveVisibilitySession).not.toHaveBeenCalled()
    expect(result.settings).toEqual(savedSettings)
    expect(store.savedSettings).toEqual(savedSettings)
    expect(store.draftSettings).toEqual(savedSettings)
    expect(store.saved).toBe(true)

    await vi.advanceTimersByTimeAsync(2000)
    expect(store.saved).toBe(false)
  })

  it('cancels the visibility session when save fails', async () => {
    setTauriInvokeHandler(async (command) => {
      if (command === 'get_settings') {
        return createAppSettings()
      }
      if (command === 'save_settings') {
        throw new Error('save failed')
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const store = useSettingsStore()
    await store.load()
    store.draftSettings = {
      ...store.draftSettings,
      max_history: 999,
    }

    await expect(store.save()).rejects.toThrow('save failed')
    expect(coordinatorMocks.cancelSettingsSaveVisibilitySession).toHaveBeenCalledOnce()
  })
})
