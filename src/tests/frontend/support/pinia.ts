import { createPinia, setActivePinia } from 'pinia'
import { useAppInfoStore } from '../../../stores/appInfo'
import { useRuntimeStore } from '../../../stores/runtime'
import { useSettingsStore } from '../../../stores/settings'
import { createAppInfo, createAppSettings, createRuntimeStatus } from './factories'
import type { AppInfo, AppSettings, RuntimeStatus } from '../../../types'

export function installTestPinia() {
  const pinia = createPinia()
  setActivePinia(pinia)
  return pinia
}

export function primeAppInfoStore(appInfo: AppInfo = createAppInfo()) {
  const store = useAppInfoStore()
  store.appInfo = appInfo
  return store
}

export function primeRuntimeStore(runtime: RuntimeStatus = createRuntimeStatus()) {
  const store = useRuntimeStore()
  store.runtime = runtime
  return store
}

export function primeSettingsStore(settings: AppSettings = createAppSettings()) {
  const store = useSettingsStore()
  store.savedSettings = { ...settings }
  store.draftSettings = { ...settings }
  return store
}
