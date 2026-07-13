import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  clearAll,
  copyEntry,
  deleteEntry,
  fetchActiveDates as fetchActiveDatesApi,
  fetchClipboardListPage,
  fetchEarliestMonth as fetchEarliestMonthApi,
  listenClipboardChanged,
  reportImageLoadFailed,
  togglePin as togglePinEntry,
  type ClipboardChangedUnlisten,
} from '../composables/clipboardApi'
import { globalNow } from '../hooks/useNow'
import type {
  ClipboardEntriesQuery,
  ClipboardListItem,
  ClipboardQueryCursor,
  ImagePreviewRepairOutcome,
} from '../types'
import {
  buildEntrySearchFilters,
  createEntrySearchCommandFilters,
  setEntrySearchCommandFilter as replaceEntrySearchCommandFilter,
  type EntrySearchCommandFilterValue,
  type EntrySearchCommandFilters,
  type EntrySearchCommandValue,
} from '../utils/entrySearchCommands'
import { useAppInfoStore } from './appInfo'

type CommittedClipboardQuery = Omit<ClipboardEntriesQuery, 'cursor' | 'limit'>

function copyCommittedQuery(query: CommittedClipboardQuery): CommittedClipboardQuery {
  return { ...query }
}

export const useClipboardViewStore = defineStore('clipboardView', () => {
  const appInfoStore = useAppInfoStore()

  const items = ref<ClipboardListItem[]>([])
  const nextCursor = ref<ClipboardQueryCursor | null>(null)
  const pinnedCount = ref(0)
  const loadedRevision = ref(0)
  const dirtyRevision = ref(0)
  const loading = ref(false)
  const loadingMore = ref(false)
  const error = ref<unknown | null>(null)
  const active = ref(false)

  const searchInput = ref('')
  const selectedDate = ref<string | null>(null)
  const searchCommandFilters = ref<EntrySearchCommandFilters>(createEntrySearchCommandFilters())
  const activeQuery = ref<CommittedClipboardQuery>({})

  const searchFilters = computed(() =>
    buildEntrySearchFilters(searchInput.value, searchCommandFilters.value),
  )
  const visibleItems = computed(() =>
    items.value.filter((item) =>
      item.visible_until === null || item.visible_until > globalNow.value,
    ),
  )
  const hasMore = computed(() => nextCursor.value !== null)

  let requestGeneration = 0
  let lifecycleGeneration = 0
  let unlisten: ClipboardChangedUnlisten | null = null
  let startPromise: Promise<void> | null = null
  let refreshPromise: Promise<void> | null = null
  let loadMorePromise: Promise<void> | null = null
  let refreshRequestVersion = 0
  let refreshAgain = false

  function pageSize(): number {
    return appInfoStore.requireAppInfo().page_size
  }

  function buildDraftQuery(): CommittedClipboardQuery {
    const query: CommittedClipboardQuery = {}
    if (searchFilters.value.text) query.text = searchFilters.value.text
    if (searchFilters.value.entryType) query.entryType = searchFilters.value.entryType
    if (searchFilters.value.tag) query.tag = searchFilters.value.tag
    if (selectedDate.value) query.date = selectedDate.value
    return query
  }

  async function fetchFirstPage(): Promise<void> {
    const generation = requestGeneration
    loading.value = true
    error.value = null

    try {
      const page = await fetchClipboardListPage({
        ...copyCommittedQuery(activeQuery.value),
        limit: pageSize(),
      })
      if (generation !== requestGeneration) return

      // A response from an older engine revision must never replace newer data.
      if (page.revision < loadedRevision.value) {
        queueRefreshRequest()
        return
      }

      items.value = [...page.items]
      nextCursor.value = page.next_cursor
      pinnedCount.value = page.pinned_count
      loadedRevision.value = page.revision
      if (page.revision >= dirtyRevision.value) {
        dirtyRevision.value = 0
      } else {
        queueRefreshRequest()
      }
    } catch (cause) {
      if (generation === requestGeneration) error.value = cause
      throw cause
    } finally {
      if (generation === requestGeneration) loading.value = false
    }
  }

  function queueRefreshRequest(): void {
    refreshRequestVersion += 1
    refreshAgain = true
  }

  async function runRefreshCycle(): Promise<void> {
    refreshAgain = false
    let coveredVersion = refreshRequestVersion
    await fetchFirstPage()
    if (!active.value) {
      refreshAgain = false
      return
    }

    // Events or query changes during a fetch collapse into one follow-up read.
    if (refreshRequestVersion !== coveredVersion) {
      refreshAgain = false
      coveredVersion = refreshRequestVersion
      await fetchFirstPage()
    }
    refreshAgain = refreshRequestVersion !== coveredVersion
  }

  function refresh(force = true): Promise<void> {
    if (force) queueRefreshRequest()

    if (refreshPromise) {
      return refreshPromise
    }
    if (loadMorePromise) {
      return loadMorePromise
        .catch(() => undefined)
        .then(() => {
          if (!active.value) return
          return refresh(false)
        })
    }

    refreshPromise = runRefreshCycle().finally(() => {
      refreshPromise = null
      // A change that raced the follow-up fetch starts a new collapsed cycle.
      if (refreshAgain && active.value) {
        void refresh(false).catch((cause) => {
          console.error('[clipboard] failed to refresh invalidated view:', cause)
        })
      }
    })
    return refreshPromise
  }

  function invalidate(revision?: number): void {
    if (revision !== undefined) {
      if (revision <= Math.max(loadedRevision.value, dirtyRevision.value)) return
      dirtyRevision.value = Math.max(dirtyRevision.value, revision)
    }

    if (!active.value) return
    void refresh().catch((cause) => {
      console.error('[clipboard] failed to refresh invalidated view:', cause)
    })
  }

  async function start(): Promise<void> {
    if (active.value && unlisten) return
    if (startPromise) {
      await startPromise
      if (!active.value || !unlisten) return start()
      return
    }

    const lifecycle = ++lifecycleGeneration
    active.value = true
    startPromise = (async () => {
      let stopListening: ClipboardChangedUnlisten
      try {
        stopListening = await listenClipboardChanged(({ revision }) => invalidate(revision))
      } catch (cause) {
        if (lifecycle === lifecycleGeneration) active.value = false
        throw cause
      }
      if (lifecycle !== lifecycleGeneration || !active.value) {
        stopListening()
        return
      }
      unlisten = stopListening
      await refresh()
    })().finally(() => {
      startPromise = null
    })
    return startPromise
  }

  function stop(): void {
    lifecycleGeneration += 1
    active.value = false
    unlisten?.()
    unlisten = null
    requestGeneration += 1
    refreshRequestVersion += 1
    refreshAgain = false
    items.value = []
    nextCursor.value = null
    pinnedCount.value = 0
    loadedRevision.value = 0
    dirtyRevision.value = 0
    loading.value = false
    loadingMore.value = false
    error.value = null
  }

  async function applyCurrentFilter(date: string | null = selectedDate.value): Promise<void> {
    selectedDate.value = date
    activeQuery.value = buildDraftQuery()
    requestGeneration += 1
    items.value = []
    nextCursor.value = null
    await refresh()
  }

  function setSearchInput(value: string): void {
    searchInput.value = value
  }

  function setSearchCommandFilter(
    command: EntrySearchCommandValue,
    value: EntrySearchCommandFilterValue | null,
  ): void {
    searchCommandFilters.value = replaceEntrySearchCommandFilter(
      searchCommandFilters.value,
      command,
      value,
    )
  }

  function clearSearchCommandFilter(command: EntrySearchCommandValue): void {
    setSearchCommandFilter(command, null)
  }

  async function clearSearch(): Promise<void> {
    searchInput.value = ''
    searchCommandFilters.value = createEntrySearchCommandFilters()
    await applyCurrentFilter(null)
  }

  function loadMore(): Promise<void> {
    if (
      loading.value ||
      loadingMore.value ||
      refreshPromise ||
      nextCursor.value === null
    ) return Promise.resolve()

    const generation = requestGeneration
    const revision = loadedRevision.value
    const refreshVersion = refreshRequestVersion
    const cursor = nextCursor.value
    loadingMore.value = true
    const operation = (async () => {
      try {
        const page = await fetchClipboardListPage({
          ...copyCommittedQuery(activeQuery.value),
          cursor,
          limit: pageSize(),
        })
        if (generation !== requestGeneration) return

        if (
          page.revision !== revision ||
          loadedRevision.value !== revision ||
          refreshRequestVersion !== refreshVersion
        ) {
          dirtyRevision.value = Math.max(dirtyRevision.value, page.revision)
          if (refreshRequestVersion === refreshVersion) queueRefreshRequest()
          return
        }

        const knownIds = new Set(items.value.map((item) => item.id))
        items.value.push(...page.items.filter((item) => !knownIds.has(item.id)))
        nextCursor.value = page.next_cursor
        pinnedCount.value = page.pinned_count
      } finally {
        loadingMore.value = false
      }
    })()
    loadMorePromise = operation.finally(() => {
      loadMorePromise = null
      if (refreshAgain && active.value) {
        void refresh(false).catch((cause) => {
          console.error('[clipboard] failed to refresh after pagination:', cause)
        })
      }
    })
    return loadMorePromise
  }

  async function copy(id: string): Promise<void> {
    try {
      await copyEntry(id)
    } finally {
      // Copy may delete a broken image entry before returning an error.
      invalidate()
    }
  }

  async function remove(id: string): Promise<void> {
    await deleteEntry(id)
    invalidate()
  }

  async function clear(): Promise<void> {
    await clearAll()
    invalidate()
  }

  async function togglePin(id: string): Promise<void> {
    await togglePinEntry(id)
    invalidate()
  }

  async function repairImagePreview(id: string): Promise<ImagePreviewRepairOutcome> {
    const outcome = await reportImageLoadFailed(id)
    if (outcome === 'repaired' || outcome === 'removed') invalidate()
    return outcome
  }

  async function fetchActiveDates(yearMonth: string): Promise<string[]> {
    return fetchActiveDatesApi(yearMonth)
  }

  async function fetchEarliestMonth(): Promise<string | null> {
    return fetchEarliestMonthApi()
  }

  return {
    items,
    visibleItems,
    nextCursor,
    pinnedCount,
    loadedRevision,
    dirtyRevision,
    loading,
    loadingMore,
    hasMore,
    error,
    active,
    searchInput,
    selectedDate,
    searchCommandFilters,
    searchFilters,
    activeQuery,
    start,
    stop,
    refresh,
    invalidate,
    applyCurrentFilter,
    loadMore,
    setSearchInput,
    setSearchCommandFilter,
    clearSearchCommandFilter,
    clearSearch,
    copy,
    remove,
    clear,
    togglePin,
    repairImagePreview,
    fetchActiveDates,
    fetchEarliestMonth,
  }
})
