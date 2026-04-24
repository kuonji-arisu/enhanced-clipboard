import { describe, expect, it } from 'vitest'
import { closeCurrentWindow } from '../../../../composables/windowApi'
import { tauriCloseWindowMock } from '../../support/tauri'

describe('windowApi', () => {
  it('wraps current-window close behind a composable API', async () => {
    await closeCurrentWindow()

    expect(tauriCloseWindowMock).toHaveBeenCalledOnce()
  })
})
