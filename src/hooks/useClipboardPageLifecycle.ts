import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardActionsStore } from '../stores/clipboardActions'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'
import { CLIPBOARD_QUERY_STALE_REASON } from '../types'
import { useClipboardViewEvents } from './useClipboardViewEvents'

export function useClipboardPageLifecycle() {
  const actionsStore = useClipboardActionsStore()
  const calendarMetaStore = useCalendarMetaStore()
  const queryStore = useClipboardQueryStore()
  const streamStore = useClipboardStreamStore()
  const viewEvents = useClipboardViewEvents()

  async function loadInitialStream() {
    await streamStore.loadInitial()
    await calendarMetaStore.refreshEarliestMonth()
  }

  async function initStreamView() {
    await viewEvents.start()
    if (streamStore.items.length === 0) {
      await loadInitialStream()
    } else {
      await calendarMetaStore.refreshEarliestMonth()
    }
  }

  async function clearAllEntries() {
    await actionsStore.clear()
    streamStore.markCleared()
    queryStore.markStale(CLIPBOARD_QUERY_STALE_REASON.CLEAR_ALL)
  }

  return {
    initStreamView,
    clearAllEntries,
  }
}
