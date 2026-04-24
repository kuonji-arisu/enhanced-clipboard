import { describe, expect, it, vi } from 'vitest'
import { EVENT_UI_RESUME, EVENT_UI_SUSPEND } from '../../../../constants'
import { listenUiLifecycleEvents } from '../../../../composables/uiLifecycleApi'
import { emitTauriEvent, tauriListenMock } from '../../support/tauri'

describe('uiLifecycleApi', () => {
  it('wraps Tauri UI lifecycle events behind one listener API', async () => {
    const onSuspend = vi.fn()
    const onResume = vi.fn()

    const unlisten = await listenUiLifecycleEvents({ onSuspend, onResume })
    await emitTauriEvent(EVENT_UI_SUSPEND, null)
    await emitTauriEvent(EVENT_UI_RESUME, null)

    expect(onSuspend).toHaveBeenCalledOnce()
    expect(onResume).toHaveBeenCalledOnce()

    unlisten()
    await emitTauriEvent(EVENT_UI_SUSPEND, null)
    await emitTauriEvent(EVENT_UI_RESUME, null)

    expect(onSuspend).toHaveBeenCalledOnce()
    expect(onResume).toHaveBeenCalledOnce()
  })

  it('cleans up the first listener if binding the second listener fails', async () => {
    const unlistenSuspend = vi.fn()
    tauriListenMock
      .mockImplementationOnce(async () => unlistenSuspend)
      .mockImplementationOnce(async () => {
        throw new Error('bind failed')
      })

    await expect(
      listenUiLifecycleEvents({
        onSuspend: vi.fn(),
        onResume: vi.fn(),
      }),
    ).rejects.toThrow('bind failed')

    expect(unlistenSuspend).toHaveBeenCalledOnce()
  })
})
