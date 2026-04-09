import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  fetchPersistedState,
  savePersistedState as savePersistedStateApi,
} from '../composables/persistedStateApi'
import type { PersistedState, PersistedStatePatch, SavePersistedResult } from '../types'

export const usePersistedStateStore = defineStore('persistedState', () => {
  const persisted = ref<PersistedState>({
    window_x: null,
    window_y: null,
    always_on_top: false,
  })
  let saveQueue: Promise<SavePersistedResult | undefined> = Promise.resolve(undefined)

  async function load() {
    persisted.value = await fetchPersistedState()
  }

  async function save(patch: PersistedStatePatch): Promise<SavePersistedResult> {
    const operation = saveQueue.then(async () => {
      const result = await savePersistedStateApi(patch)
      persisted.value = result.persisted
      return result
    })
    saveQueue = operation.catch(() => undefined)
    return operation
  }

  async function toggleAlwaysOnTop() {
    return save({ always_on_top: !persisted.value.always_on_top })
  }

  return {
    persisted,
    load,
    save,
    toggleAlwaysOnTop,
  }
})
