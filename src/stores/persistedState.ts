import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  fetchPersistedState,
  savePersistedState as savePersistedStateApi,
} from '../composables/persistedStateApi'
import type { PersistedState, PersistedStatePatch } from '../types'

export const usePersistedStateStore = defineStore('persistedState', () => {
  const persistedState = ref<PersistedState>({
    window_x: null,
    window_y: null,
    always_on_top: false,
  })
  let toggleQueue: Promise<void> = Promise.resolve()

  async function load() {
    persistedState.value = await fetchPersistedState()
  }

  async function save(patch: PersistedStatePatch) {
    const operation = toggleQueue.then(async () => {
      const next = { ...persistedState.value, ...patch }
      await savePersistedStateApi(patch)
      persistedState.value = next
    })
    toggleQueue = operation.catch(() => {})
    return operation
  }

  async function toggleAlwaysOnTop() {
    return save({ always_on_top: !persistedState.value.always_on_top })
  }

  return {
    persistedState,
    load,
    save,
    toggleAlwaysOnTop,
  }
})
