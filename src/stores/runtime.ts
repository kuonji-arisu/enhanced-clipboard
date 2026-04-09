import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  fetchRuntimeStatus,
  listenRuntimeStatusPatches,
} from '../composables/runtimeApi'
import type { RuntimeStatus, RuntimeStatusPatch } from '../types'

const DEFAULT_RUNTIME_STATUS: RuntimeStatus = {
  clipboard_capture_available: true,
  system_theme: 'light',
}

function mergeRuntimePatch(
  current: RuntimeStatus,
  patch: RuntimeStatusPatch,
): RuntimeStatus {
  return {
    ...current,
    ...patch,
  }
}

export const useRuntimeStore = defineStore('runtime', () => {
  const runtime = ref<RuntimeStatus>({ ...DEFAULT_RUNTIME_STATUS })
  const loading = ref(false)
  let pendingLoad: Promise<void> | null = null
  let pendingBind: Promise<void> | null = null
  let pendingStart: Promise<void> | null = null
  let unlisten: UnlistenFn | null = null
  let hydrating = false
  const queuedPatches: RuntimeStatusPatch[] = []

  async function load() {
    if (!pendingLoad) {
      loading.value = true
      pendingLoad = fetchRuntimeStatus()
        .then((snapshot) => {
          runtime.value = snapshot
        })
        .finally(() => {
          loading.value = false
          pendingLoad = null
        })
    }

    await pendingLoad
  }

  function applyPatch(patch: RuntimeStatusPatch) {
    if (Object.keys(patch).length === 0) {
      return
    }
    if (hydrating) {
      queuedPatches.push(patch)
      return
    }
    runtime.value = mergeRuntimePatch(runtime.value, patch)
  }

  async function bindEvents() {
    if (unlisten) {
      return
    }

    if (!pendingBind) {
      pendingBind = listenRuntimeStatusPatches((patch) => {
        applyPatch(patch)
      })
        .then((dispose) => {
          unlisten = dispose
        })
        .finally(() => {
          pendingBind = null
        })
    }

    await pendingBind
  }

  async function start() {
    if (!pendingStart) {
      pendingStart = (async () => {
        hydrating = true
        try {
          await bindEvents()
          await load()

          const patches = queuedPatches.splice(0, queuedPatches.length)
          for (const patch of patches) {
            runtime.value = mergeRuntimePatch(runtime.value, patch)
          }
        } finally {
          hydrating = false
          pendingStart = null
        }
      })()
    }

    await pendingStart
  }

  function stop() {
    unlisten?.()
    unlisten = null
    hydrating = false
    queuedPatches.length = 0
  }

  return {
    runtime,
    loading,
    load,
    applyPatch,
    bindEvents,
    start,
    stop,
  }
})
