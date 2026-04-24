import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { EVENT_UI_RESUME, EVENT_UI_SUSPEND } from '../constants'

export interface UiLifecycleEventHandlers {
  onSuspend: () => void | Promise<void>
  onResume: () => void | Promise<void>
}

export async function listenUiLifecycleEvents(
  handlers: UiLifecycleEventHandlers,
): Promise<UnlistenFn> {
  const unlistenSuspend = await listen(EVENT_UI_SUSPEND, () => handlers.onSuspend())
  let unlistenResume: UnlistenFn
  try {
    unlistenResume = await listen(EVENT_UI_RESUME, () => handlers.onResume())
  } catch (error) {
    unlistenSuspend()
    throw error
  }

  return () => {
    unlistenSuspend()
    unlistenResume()
  }
}
