import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  fetchPersistedState,
  setAlwaysOnTop as setAlwaysOnTopApi,
} from '../composables/persistedStateApi'
import type { PersistedState } from '../types'

export const usePersistedStateStore = defineStore('persistedState', () => {
  const persistedState = ref<PersistedState>({
    window_x: null,
    window_y: null,
    always_on_top: false,
  })

  async function load() {
    persistedState.value = await fetchPersistedState()
  }

  async function toggleAlwaysOnTop() {
    const next = !persistedState.value.always_on_top
    await setAlwaysOnTopApi(next)
    persistedState.value = { ...persistedState.value, always_on_top: next }
  }

  return {
    persistedState,
    load,
    toggleAlwaysOnTop,
  }
})
