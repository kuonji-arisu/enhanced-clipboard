import { vi } from 'vitest'

type InvokeArgs = Record<string, unknown> | undefined
type InvokeHandler = (command: string, args: InvokeArgs) => unknown | Promise<unknown>
type EventCallback = (event: { payload: unknown }) => void

const tauriState = vi.hoisted(() => ({
  listeners: new Map<string, Set<EventCallback>>(),
  invokeHandler: null as InvokeHandler | null,
}))

const defaultConvertFileSrc = (path: string) => `asset://${path.replace(/\\/g, '/')}`

export const tauriInvokeMock = vi.fn(async (command: string, args?: InvokeArgs) => {
  if (!tauriState.invokeHandler) {
    throw new Error(`No Tauri invoke handler configured for "${command}"`)
  }
  return tauriState.invokeHandler(command, args)
})

export const tauriConvertFileSrcMock = vi.fn(defaultConvertFileSrc)
export const tauriCloseWindowMock = vi.fn(async () => undefined)

export const tauriListenMock = vi.fn(async (event: string, callback: EventCallback) => {
  const callbacks = tauriState.listeners.get(event) ?? new Set<EventCallback>()
  callbacks.add(callback)
  tauriState.listeners.set(event, callbacks)

  return () => {
    const activeCallbacks = tauriState.listeners.get(event)
    if (!activeCallbacks) return
    activeCallbacks.delete(callback)
    if (activeCallbacks.size === 0) {
      tauriState.listeners.delete(event)
    }
  }
})

vi.mock('@tauri-apps/api/core', () => ({
  invoke: tauriInvokeMock,
  convertFileSrc: tauriConvertFileSrcMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: tauriListenMock,
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    close: tauriCloseWindowMock,
  }),
}))

export function resetTauriMocks(): void {
  tauriState.listeners.clear()
  tauriState.invokeHandler = null

  tauriInvokeMock.mockReset()
  tauriInvokeMock.mockImplementation(async (command: string, args?: InvokeArgs) => {
    if (!tauriState.invokeHandler) {
      throw new Error(`No Tauri invoke handler configured for "${command}"`)
    }
    return tauriState.invokeHandler(command, args)
  })

  tauriConvertFileSrcMock.mockReset()
  tauriConvertFileSrcMock.mockImplementation(defaultConvertFileSrc)

  tauriCloseWindowMock.mockReset()
  tauriCloseWindowMock.mockImplementation(async () => undefined)

  tauriListenMock.mockReset()
  tauriListenMock.mockImplementation(async (event: string, callback: EventCallback) => {
    const callbacks = tauriState.listeners.get(event) ?? new Set<EventCallback>()
    callbacks.add(callback)
    tauriState.listeners.set(event, callbacks)

    return () => {
      const activeCallbacks = tauriState.listeners.get(event)
      if (!activeCallbacks) return
      activeCallbacks.delete(callback)
      if (activeCallbacks.size === 0) {
        tauriState.listeners.delete(event)
      }
    }
  })
}

export function setTauriInvokeHandler(handler: InvokeHandler): void {
  tauriState.invokeHandler = handler
}

export async function emitTauriEvent(event: string, payload: unknown): Promise<void> {
  const callbacks = [...(tauriState.listeners.get(event) ?? [])]
  for (const callback of callbacks) {
    await callback({ payload })
  }
}
