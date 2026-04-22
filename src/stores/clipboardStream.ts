import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  fetchClipboardListItems,
  listenClipboardEvents,
} from '../composables/clipboardApi'
import { globalNow } from '../hooks/useNow'
import { useAppInfoStore } from './appInfo'
import { useSettingsStore } from './settings'
import { useCalendarMetaStore } from './calendarMeta'
import { useClipboardQueryStore } from './clipboardQuery'
import {
  compareListItems,
  lastNonPinnedListItem,
  removeListItem,
  upsertListItem,
} from './clipboardListUtils'
import { CLIPBOARD_QUERY_STALE_REASON, type ClipboardListItem } from '../types'

// Default history stream state. This is the main list source of truth and is
// allowed to apply view-facing stream events incrementally; query snapshots use
// stale signaling instead of mirroring this reconciliation.
export const useClipboardStreamStore = defineStore('clipboardStream', () => {
  const items = ref<ClipboardListItem[]>([])
  const loading = ref(false)
  const loadingMore = ref(false)
  const hasMore = ref(true)

  let unlisten: UnlistenFn | null = null
  let listRevision = 0

  const appInfoStore = useAppInfoStore()
  const settingsStore = useSettingsStore()
  const calendarMetaStore = useCalendarMetaStore()
  const queryStore = useClipboardQueryStore()

  const pinnedCount = computed(() => items.value.filter((item) => item.is_pinned).length)

  function pageSize(): number {
    return appInfoStore.requireAppInfo().page_size
  }

  function removeExpired(now: number): void {
    const ttl = settingsStore.savedSettings.expiry_seconds
    if (ttl <= 0) return
    const cutoff = now - ttl
    const expired = items.value.filter((item) => !item.is_pinned && item.created_at < cutoff)
    for (const item of expired) {
      removeListItem(items.value, item.id)
    }
  }

  watch(globalNow, (now) => {
    removeExpired(now)
  })

  async function loadInitial() {
    const revision = ++listRevision
    loading.value = true
    loadingMore.value = false
    hasMore.value = true
    try {
      const result = await fetchClipboardListItems({ limit: pageSize() })
      if (revision !== listRevision) return
      items.value = [...result]
      const normalCount = result.filter((item) => !item.is_pinned).length
      hasMore.value = normalCount === pageSize()
      await calendarMetaStore.refreshEarliestMonth()
    } finally {
      if (revision === listRevision) loading.value = false
    }
  }

  async function loadMore() {
    if (loadingMore.value || !hasMore.value) return
    const revision = listRevision
    loadingMore.value = true
    try {
      const last = lastNonPinnedListItem(items.value)
      if (!last) {
        hasMore.value = false
        return
      }
      const result = await fetchClipboardListItems({
        cursor: {
          createdAt: last.created_at,
          id: last.id,
        },
        limit: pageSize(),
      })
      if (revision !== listRevision) return
      for (const item of result) {
        upsertListItem(items.value, item)
      }
      hasMore.value = result.length === pageSize()
    } finally {
      loadingMore.value = false
    }
  }

  function handleStreamItemAdded(item: ClipboardListItem) {
    upsertListItem(items.value, item)
    queryStore.markStale(CLIPBOARD_QUERY_STALE_REASON.ENTRY_CREATED)
    calendarMetaStore.notifyCalendarDatesChanged()
  }

  function shouldApplyUnknownStreamUpdate(item: ClipboardListItem): boolean {
    if (item.is_pinned || !hasMore.value) return true

    const lastLoadedNormal = lastNonPinnedListItem(items.value)
    if (!lastLoadedNormal) return items.value.length === 0

    return compareListItems(item, lastLoadedNormal) < 0
  }

  function handleStreamItemUpdated(item: ClipboardListItem) {
    const alreadyLoaded = items.value.some((current) => current.id === item.id)
    if (!alreadyLoaded && !shouldApplyUnknownStreamUpdate(item)) return
    upsertListItem(items.value, item)
    queryStore.markStale(CLIPBOARD_QUERY_STALE_REASON.ENTRY_UPDATED)
  }

  function handleEntriesRemoved(ids: string[]) {
    for (const id of ids) {
      removeListItem(items.value, id)
    }
    queryStore.removeKnownIds(ids)
    queryStore.markStale(CLIPBOARD_QUERY_STALE_REASON.ENTRIES_REMOVED)
    void calendarMetaStore.refreshCalendarMeta().catch((error) => {
      console.error('[clipboard] failed to refresh calendar metadata:', error)
    })
  }

  function markCleared() {
    items.value = []
    hasMore.value = false
  }

  async function startListening() {
    if (unlisten) return
    unlisten = await listenClipboardEvents({
      onStreamItemAdded: handleStreamItemAdded,
      onStreamItemUpdated: handleStreamItemUpdated,
      onEntriesRemoved: handleEntriesRemoved,
      onQueryResultsStale: queryStore.markStale,
    })
  }

  async function init() {
    await startListening()
    if (items.value.length === 0) {
      await loadInitial()
    } else {
      await calendarMetaStore.refreshEarliestMonth()
    }
  }

  return {
    items,
    loading,
    loadingMore,
    hasMore,
    pinnedCount,
    init,
    loadInitial,
    loadMore,
    markCleared,
    startListening,
  }
})
