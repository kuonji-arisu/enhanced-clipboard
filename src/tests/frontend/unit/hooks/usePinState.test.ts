import { beforeEach, describe, expect, it } from 'vitest'
import { usePinState } from '../../../../hooks/usePinState'
import { useNoticeStore } from '../../../../stores/notice'
import { usePersistedStateStore } from '../../../../stores/persistedState'
import { createAppInfo } from '../../support/factories'
import { installTestPinia, primeAppInfoStore } from '../../support/pinia'
import { setTauriInvokeHandler } from '../../support/tauri'

describe('usePinState', () => {
  beforeEach(() => {
    installTestPinia()
    primeAppInfoStore(createAppInfo())
  })

  it('suppresses duplicate always-on-top toggles while a previous save is in flight', async () => {
    const commands: string[] = []
    let resolveSave: ((value: {
      persisted: { window_x: null, window_y: null, always_on_top: boolean },
      effects: { always_on_top: { ok: boolean } },
    }) => void) | null = null

    setTauriInvokeHandler((command) => {
      commands.push(command)
      if (command === 'save_persisted') {
        return new Promise((resolve) => {
          resolveSave = resolve as typeof resolveSave
        })
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const persistedStateStore = usePersistedStateStore()
    const pinState = usePinState()
    const firstToggle = pinState.togglePin()
    const secondToggle = pinState.togglePin()

    await Promise.resolve()
    expect(commands).toEqual(['save_persisted'])
    expect(pinState.pinning.value).toBe(true)

    resolveSave?.({
      persisted: {
        window_x: null,
        window_y: null,
        always_on_top: true,
      },
      effects: {
        always_on_top: { ok: true },
      },
    })
    await Promise.all([firstToggle, secondToggle])

    expect(pinState.pinning.value).toBe(false)
    expect(persistedStateStore.persisted.always_on_top).toBe(true)
  })

  it('surfaces always-on-top effect failures through the shared notice path', async () => {
    setTauriInvokeHandler(async (command) => {
      if (command === 'save_persisted') {
        return {
          persisted: {
            window_x: null,
            window_y: null,
            always_on_top: false,
          },
          effects: {
            always_on_top: {
              ok: false,
              error: 'Cannot pin window',
            },
          },
        }
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const noticeStore = useNoticeStore()
    const pinState = usePinState()

    await pinState.togglePin()

    expect(noticeStore.showDialog).toBe(true)
    expect(noticeStore.dialogMessage).toBe('Cannot pin window')
  })
})
