import { useClipboardActionsStore } from '../stores/clipboardActions'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'
import { useClipboardStreamBootstrap } from './useClipboardStreamBootstrap'
import { useClipboardTtlVisibility } from './useClipboardTtlVisibility'
import { useClipboardViewEvents } from './useClipboardViewEvents'

export function useClipboardPageLifecycle() {
  const actionsStore = useClipboardActionsStore()
  const queryStore = useClipboardQueryStore()
  const streamStore = useClipboardStreamStore()
  const streamBootstrap = useClipboardStreamBootstrap()
  const ttlVisibility = useClipboardTtlVisibility()
  const viewEvents = useClipboardViewEvents()

  async function loadCurrentViewFirstPage() {
    await queryStore.applySearch(queryStore.selectedDate)
    if (queryStore.isDefaultView) {
      await streamBootstrap.loadInitialStream()
    }
  }

  async function initStreamView() {
    ttlVisibility.start()
    await viewEvents.start()
    await loadCurrentViewFirstPage()
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
    await loadCurrentViewFirstPage()
  }

  return {
    initStreamView,
    clearAllEntries,
    releaseViewCache,
    resumeView,
  }
}
