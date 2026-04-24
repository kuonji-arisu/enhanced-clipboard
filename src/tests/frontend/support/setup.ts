import { afterEach, beforeEach, vi } from 'vitest'
import { resetTauriMocks } from './tauri'

beforeEach(() => {
  resetTauriMocks()
  document.body.innerHTML = ''
  document.documentElement.removeAttribute('data-theme')

  if (!('matchMedia' in window)) {
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    })
  }

  if (!('ResizeObserver' in globalThis)) {
    class ResizeObserverMock {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }

    Object.defineProperty(globalThis, 'ResizeObserver', {
      writable: true,
      value: ResizeObserverMock,
    })
  }
})

afterEach(() => {
  vi.useRealTimers()
  document.body.innerHTML = ''
  document.documentElement.removeAttribute('data-theme')
})
