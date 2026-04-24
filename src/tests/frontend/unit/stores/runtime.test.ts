import { describe, expect, it } from 'vitest'
import { EVENT_RUNTIME_STATUS_UPDATED } from '../../../../constants'
import { useRuntimeStore } from '../../../../stores/runtime'
import { createRuntimeStatus } from '../../support/factories'
import { installTestPinia } from '../../support/pinia'
import { emitTauriEvent, setTauriInvokeHandler } from '../../support/tauri'

describe('runtime store', () => {
  it('hydrates from the backend and applies runtime patches', async () => {
    installTestPinia()
    setTauriInvokeHandler(async (command) => {
      if (command === 'get_runtime_status') {
        return createRuntimeStatus({ system_theme: 'dark' })
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const store = useRuntimeStore()
    await store.start()
    await emitTauriEvent(EVENT_RUNTIME_STATUS_UPDATED, {
      clipboard_capture_available: false,
    })

    expect(store.runtime.system_theme).toBe('dark')
    expect(store.runtime.clipboard_capture_available).toBe(false)
  })

  it('binds the runtime listener once and ignores patches after stop', async () => {
    installTestPinia()

    let loadCount = 0
    setTauriInvokeHandler(async (command) => {
      if (command !== 'get_runtime_status') {
        throw new Error(`unexpected command: ${command}`)
      }
      loadCount += 1
      return createRuntimeStatus({ system_theme: 'light' })
    })

    const store = useRuntimeStore()
    await Promise.all([store.start(), store.start()])

    expect(loadCount).toBe(1)

    store.stop()
    await emitTauriEvent(EVENT_RUNTIME_STATUS_UPDATED, {
      system_theme: 'dark',
      clipboard_capture_available: false,
    })

    expect(store.runtime).toEqual({
      clipboard_capture_available: true,
      system_theme: 'light',
    })
  })

  it('queues patches received during hydration until the initial snapshot loads', async () => {
    installTestPinia()

    let releaseLoad!: (value: ReturnType<typeof createRuntimeStatus>) => void
    const loadPromise = new Promise<ReturnType<typeof createRuntimeStatus>>((resolve) => {
      releaseLoad = resolve
    })
    setTauriInvokeHandler(async (command) => {
      if (command !== 'get_runtime_status') {
        throw new Error(`unexpected command: ${command}`)
      }
      return loadPromise
    })

    const store = useRuntimeStore()
    const startPromise = store.start()
    await Promise.resolve()
    await Promise.resolve()

    await emitTauriEvent(EVENT_RUNTIME_STATUS_UPDATED, {
      clipboard_capture_available: false,
    })

    releaseLoad(createRuntimeStatus({ system_theme: 'dark' }))
    await startPromise

    expect(store.runtime).toEqual({
      clipboard_capture_available: false,
      system_theme: 'dark',
    })
  })
})
