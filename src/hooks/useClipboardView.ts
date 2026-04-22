import { computed } from 'vue'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'

export function useClipboardView() {
  const queryStore = useClipboardQueryStore()
  const streamStore = useClipboardStreamStore()

  const entries = computed(() =>
    queryStore.isSnapshotView ? queryStore.items : streamStore.items,
  )
  const loading = computed(() =>
    queryStore.isSnapshotView ? queryStore.loading : streamStore.loading,
  )
  const loadingMore = computed(() =>
    queryStore.isSnapshotView ? queryStore.loadingMore : streamStore.loadingMore,
  )
  const hasMore = computed(() =>
    queryStore.isSnapshotView ? queryStore.hasMore : streamStore.hasMore,
  )

  async function applyCurrentFilter(date: string | null = queryStore.selectedDate) {
    await queryStore.applySearch(date)
    if (!queryStore.isSnapshotView) {
      await streamStore.loadInitial()
    }
  }

  async function clearSearch() {
    await queryStore.clearSearch()
    await streamStore.loadInitial()
  }

  async function loadMore() {
    if (queryStore.isSnapshotView) {
      await queryStore.loadMore()
      return
    }
    await streamStore.loadMore()
  }

  async function refreshStaleSnapshot() {
    await queryStore.refreshSnapshot()
  }

  return {
    entries,
    loading,
    loadingMore,
    hasMore,
    queryStore,
    streamStore,
    applyCurrentFilter,
    clearSearch,
    loadMore,
    refreshStaleSnapshot,
  }
}
