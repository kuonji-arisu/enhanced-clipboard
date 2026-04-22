import { computed } from 'vue'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'

export type ClipboardViewMode = 'stream' | 'snapshot'

export function useClipboardCurrentList() {
  const queryStore = useClipboardQueryStore()
  const streamStore = useClipboardStreamStore()

  const viewMode = computed<ClipboardViewMode>(() =>
    queryStore.isSnapshotView ? 'snapshot' : 'stream',
  )
  const isStreamView = computed(() => viewMode.value === 'stream')
  const isSnapshotView = computed(() => viewMode.value === 'snapshot')

  const entries = computed(() =>
    isSnapshotView.value ? queryStore.items : streamStore.items,
  )
  const loading = computed(() =>
    isSnapshotView.value ? queryStore.loading : streamStore.loading,
  )
  const loadingMore = computed(() =>
    isSnapshotView.value ? queryStore.loadingMore : streamStore.loadingMore,
  )
  const hasMore = computed(() =>
    isSnapshotView.value ? queryStore.hasMore : streamStore.hasMore,
  )
  const snapshotStale = computed(() => isSnapshotView.value && queryStore.stale)
  const highlightQuery = computed(() => queryStore.searchFilters.text.trim())
  const pinnedCount = computed(() => streamStore.pinnedCount)

  async function loadMore() {
    if (isSnapshotView.value) {
      await queryStore.loadMore()
      return
    }
    await streamStore.loadMore()
  }

  async function refreshStaleSnapshot() {
    await queryStore.refreshSnapshot()
  }

  return {
    viewMode,
    isStreamView,
    isSnapshotView,
    entries,
    loading,
    loadingMore,
    hasMore,
    snapshotStale,
    highlightQuery,
    pinnedCount,
    loadMore,
    refreshStaleSnapshot,
  }
}
