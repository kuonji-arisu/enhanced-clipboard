import { defineStore } from 'pinia'
import { ref } from 'vue'
import { fetchAppInfo } from '../composables/appInfoApi'
import type { AppInfo } from '../types'

export const useAppInfoStore = defineStore('appInfo', () => {
  const appInfo = ref<AppInfo | null>(null)
  const loading = ref(false)
  let pendingLoad: Promise<void> | null = null

  async function load() {
    if (appInfo.value) return
    if (!pendingLoad) {
      loading.value = true
      pendingLoad = fetchAppInfo()
        .then((value) => {
          appInfo.value = value
        })
        .finally(() => {
          loading.value = false
          pendingLoad = null
        })
    }
    await pendingLoad
  }

  function requireAppInfo(): AppInfo {
    if (!appInfo.value) {
      throw new Error('App info is not loaded')
    }
    return appInfo.value
  }

  return { appInfo, loading, load, requireAppInfo }
})
