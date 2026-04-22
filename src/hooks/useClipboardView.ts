import { computed } from 'vue'
import { useClipboardActionsStore } from '../stores/clipboardActions'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'
import { useCalendarMetaStore } from '../stores/calendarMeta'
import { CLIPBOARD_QUERY_STALE_REASON } from '../types'

export type ClipboardViewMode = 'stream' | 'snapshot'

export function useClipboardView() {
  const actionsStore = useClipboardActionsStore()
  const calendarMetaStore = useCalendarMetaStore()
  const queryStore = useClipboardQueryStore()
  const streamStore = useClipboardStreamStore()

  // Current list mode is a view-level concept: stream is the default history
  // timeline, while snapshot is a query result that can become stale.
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

  const searchInput = computed({
    get: () => queryStore.searchInput,
    set: queryStore.setSearchInput,
  })
  const selectedDate = computed(() => queryStore.selectedDate)
  const searchCommandFilters = computed(() => queryStore.searchCommandFilters)
  const earliestMonth = computed(() => calendarMetaStore.earliestMonth)
  const calendarRevision = computed(() => calendarMetaStore.calendarRevision)

  async function applyCurrentFilter(date: string | null = queryStore.selectedDate) {
    await queryStore.applySearch(date)
    if (isStreamView.value) {
      await streamStore.loadInitial()
    }
  }

  async function clearSearch() {
    await queryStore.clearSearch()
    await streamStore.loadInitial()
  }

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

  async function initStreamView() {
    await streamStore.init()
  }

  async function clearAllEntries() {
    await actionsStore.clear()
    streamStore.markCleared()
    queryStore.markStale(CLIPBOARD_QUERY_STALE_REASON.CLEAR_ALL)
  }

  async function refreshCalendarMeta() {
    await calendarMetaStore.refreshCalendarMeta()
  }

  async function fetchActiveDates(yearMonth: string) {
    return calendarMetaStore.fetchActiveDates(yearMonth)
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
    searchInput,
    selectedDate,
    searchCommandFilters,
    earliestMonth,
    calendarRevision,
    setSearchInput: queryStore.setSearchInput,
    setSearchCommandFilter: queryStore.setSearchCommandFilter,
    clearSearchCommandFilter: queryStore.clearSearchCommandFilter,
    applyCurrentFilter,
    clearSearch,
    loadMore,
    refreshStaleSnapshot,
    initStreamView,
    clearAllEntries,
    refreshCalendarMeta,
    fetchActiveDates,
  }
}
