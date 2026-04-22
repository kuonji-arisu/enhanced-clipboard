import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardActionsStore } from '../stores/clipboardActions'
import { useClipboardStreamStore } from '../stores/clipboardStream'
import { useClipboardStreamBootstrap } from './useClipboardStreamBootstrap'
import { useClipboardTtlVisibility } from './useClipboardTtlVisibility'
import { useClipboardViewEvents } from './useClipboardViewEvents'

export function useClipboardPageLifecycle() {
  const actionsStore = useClipboardActionsStore()
  const calendarMetaStore = useCalendarMetaStore()
  const streamStore = useClipboardStreamStore()
  const streamBootstrap = useClipboardStreamBootstrap()
  const ttlVisibility = useClipboardTtlVisibility()
  const viewEvents = useClipboardViewEvents()

  async function initStreamView() {
    ttlVisibility.start()
    await viewEvents.start()
    if (streamStore.items.length === 0) {
      await streamBootstrap.loadInitialStream()
    } else {
      await calendarMetaStore.refreshEarliestMonth()
    }
  }

  async function clearAllEntries() {
    await actionsStore.clear()
  }

  return {
    initStreamView,
    clearAllEntries,
  }
}
