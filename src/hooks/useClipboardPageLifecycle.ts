import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardActionsStore } from '../stores/clipboardActions'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'
import { useClipboardStreamBootstrap } from './useClipboardStreamBootstrap'
import { useClipboardTtlVisibility } from './useClipboardTtlVisibility'
import { useClipboardViewEvents } from './useClipboardViewEvents'

export function useClipboardPageLifecycle() {
  const actionsStore = useClipboardActionsStore()
  const calendarMetaStore = useCalendarMetaStore()
  const queryStore = useClipboardQueryStore()
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

  function releaseViewCache() {
    viewEvents.stop()
    streamStore.releaseLoadedItems()
    queryStore.releaseLoadedItems()
  }

  async function resumeView() {
    ttlVisibility.start()
    await viewEvents.start()
    await queryStore.applySearch(queryStore.selectedDate)
    if (queryStore.isDefaultView) {
      await streamBootstrap.loadInitialStream()
    }
  }

  return {
    initStreamView,
    clearAllEntries,
    releaseViewCache,
    resumeView,
  }
}
