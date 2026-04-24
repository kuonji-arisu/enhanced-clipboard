import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { fetchClipboardListItem, fetchClipboardListItems } from '../composables/clipboardApi'
import { useAppInfoStore } from './appInfo'
import { useSettingsStore } from './settings'
import {
  buildEntrySearchFilters,
  createEntrySearchCommandFilters,
  setEntrySearchCommandFilter,
  type EntrySearchCommandFilterValue,
  type EntrySearchCommandFilters,
  type EntrySearchCommandValue,
} from '../utils/entrySearchCommands'
import {
  lastNonPinnedListItem,
  removeListItem,
  upsertListItem,
} from './clipboardListUtils'
import { removeExpiredListItems } from './clipboardTtlVisibility'
import { CLIPBOARD_QUERY_STALE_REASON } from '../types'
import type {
  ClipboardEntriesQuery,
  ClipboardListItem,
  ClipboardQueryStaleReason,
  ClipboardQueryCursor,
} from '../types'

// Snapshot query state for search/filter/date/tag views.
// These results can be paginated and marked stale, but they deliberately do
// not replay every stream event as precise membership reconciliation.
export const useClipboardQueryStore = defineStore('clipboardQuery', () => {
  const items = ref<ClipboardListItem[]>([])
  const loading = ref(false)
  const loadingMore = ref(false)
  const hasMore = ref(true)
  const stale = ref(false)
  const staleReason = ref<ClipboardQueryStaleReason | null>(null)
  const searchInput = ref('')
  const selectedDate = ref<string | null>(null)
  const searchCommandFilters = ref<EntrySearchCommandFilters>(createEntrySearchCommandFilters())
  const activeQuery = ref<ClipboardEntriesQuery>({})

  let listRevision = 0
  const appInfoStore = useAppInfoStore()
  const settingsStore = useSettingsStore()

  const searchFilters = computed(() =>
    buildEntrySearchFilters(searchInput.value, searchCommandFilters.value),
  )

  const isDefaultView = computed(() => isDefaultQuery(activeQuery.value))
  const isSnapshotView = computed(() => !isDefaultView.value)

  function pageSize(): number {
    return appInfoStore.requireAppInfo().page_size
  }

  function buildFilterFields() {
    const text = searchFilters.value.text || undefined
    const entryType = searchFilters.value.entryType || undefined
    const tag = searchFilters.value.tag || undefined
    const date = selectedDate.value || undefined

    return { text, entryType, tag, date }
  }

  function buildEntriesQuery(cursor?: ClipboardQueryCursor): ClipboardEntriesQuery {
    return {
      ...buildFilterFields(),
      cursor,
      limit: pageSize(),
    }
  }

  function buildActiveQuery(cursor?: ClipboardQueryCursor): ClipboardEntriesQuery {
    return {
      text: activeQuery.value.text,
      entryType: activeQuery.value.entryType,
      tag: activeQuery.value.tag,
      date: activeQuery.value.date,
      cursor,
      limit: pageSize(),
    }
  }

  function captureQuery(query: ClipboardEntriesQuery): ClipboardEntriesQuery {
    return {
      text: query.text,
      entryType: query.entryType,
      tag: query.tag,
      date: query.date,
    }
  }

  function isDefaultQuery(query: ClipboardEntriesQuery): boolean {
    return !query.text && !query.entryType && !query.tag && !query.date && !query.cursor
  }

  function replaceItems(nextItems: ClipboardListItem[]): void {
    items.value = [...nextItems]
  }

  function resetSnapshotState(): void {
    listRevision += 1
    activeQuery.value = {}
    items.value = []
    loading.value = false
    loadingMore.value = false
    hasMore.value = false
    stale.value = false
    staleReason.value = null
  }

  function releaseLoadedItems(): void {
    listRevision += 1
    items.value = []
    loading.value = false
    loadingMore.value = false
    hasMore.value = true
  }

  function removeExpired(now: number): string[] {
    if (!isSnapshotView.value) return []
    const removedIds = removeExpiredListItems(
      items.value,
      now,
      settingsStore.savedSettings.expiry_seconds,
    )
    if (removedIds.length === 0) return []
    markStale(CLIPBOARD_QUERY_STALE_REASON.TTL_EXPIRED)
    return removedIds
  }

  async function loadSnapshot(query: ClipboardEntriesQuery) {
    const revision = ++listRevision
    loading.value = true
    loadingMore.value = false
    try {
      const result = await fetchClipboardListItems(query)
      if (revision !== listRevision) return
      activeQuery.value = captureQuery(query)
      replaceItems(result)
      const normalCount = result.filter((item) => !item.is_pinned).length
      hasMore.value = normalCount === pageSize()
      stale.value = false
      staleReason.value = null
    } finally {
      if (revision === listRevision) loading.value = false
    }
  }

  async function applySearch(date: string | null = selectedDate.value) {
    selectedDate.value = date
    const query = buildEntriesQuery()
    if (isDefaultQuery(query)) {
      resetSnapshotState()
      return
    }

    await loadSnapshot(query)
  }

  async function loadMore() {
    if (!isSnapshotView.value || stale.value || loadingMore.value || !hasMore.value) return
    const revision = listRevision
    loadingMore.value = true
    try {
      const last = lastNonPinnedListItem(items.value)
      if (!last) {
        hasMore.value = false
        return
      }
      const result = await fetchClipboardListItems(
        buildActiveQuery({
          createdAt: last.created_at,
          id: last.id,
        }),
      )
      if (revision !== listRevision) return
      for (const item of result) {
        upsertListItem(items.value, item)
      }
      hasMore.value = result.length === pageSize()
    } finally {
      if (revision === listRevision) loadingMore.value = false
    }
  }

  function setSearchInput(input: string) {
    searchInput.value = input
  }

  function setSearchCommandFilter(
    command: EntrySearchCommandValue,
    value: EntrySearchCommandFilterValue | null,
  ): void {
    searchCommandFilters.value = setEntrySearchCommandFilter(
      searchCommandFilters.value,
      command,
      value,
    )
  }

  function clearSearchCommandFilter(command: EntrySearchCommandValue) {
    setSearchCommandFilter(command, null)
  }

  async function clearSearch() {
    searchInput.value = ''
    searchCommandFilters.value = createEntrySearchCommandFilters()
    selectedDate.value = null
    await applySearch(null)
  }

  function markStale(reason: ClipboardQueryStaleReason) {
    if (!isSnapshotView.value) return
    stale.value = true
    staleReason.value = reason
  }

  function removeKnownIds(ids: string[]) {
    if (!isSnapshotView.value) return
    for (const id of ids) {
      removeListItem(items.value, id)
    }
  }

  async function refreshKnownItem(id: string) {
    if (!isSnapshotView.value) return
    if (!items.value.some((current) => current.id === id)) return
    const revision = listRevision
    const item = await fetchClipboardListItem(id, buildActiveQuery())
    if (revision !== listRevision || !isSnapshotView.value) return
    if (item) {
      upsertListItem(items.value, item)
    } else {
      removeListItem(items.value, id)
    }
  }

  async function refreshSnapshot() {
    if (!isSnapshotView.value) return
    // Refresh intentionally returns to the first page for the active filters.
    await loadSnapshot(buildActiveQuery())
  }

  return {
    items,
    loading,
    loadingMore,
    hasMore,
    stale,
    staleReason,
    searchInput,
    selectedDate,
    searchCommandFilters,
    searchFilters,
    activeQuery,
    isDefaultView,
    isSnapshotView,
    releaseLoadedItems,
    applySearch,
    loadMore,
    setSearchInput,
    setSearchCommandFilter,
    clearSearchCommandFilter,
    clearSearch,
    markStale,
    removeKnownIds,
    removeExpired,
    refreshKnownItem,
    refreshSnapshot,
  }
})
