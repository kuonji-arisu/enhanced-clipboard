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
  lastNonPinnedListItem,
  removeListItem,
  upsertListItem,
} from './clipboardListUtils'
import type { ClipboardListItem } from '../types'

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
    queryStore.markStale('entry_created')
    calendarMetaStore.notifyCalendarDatesChanged()
  }

  function handleStreamItemUpdated(item: ClipboardListItem) {
    if (!items.value.some((current) => current.id === item.id)) return
    upsertListItem(items.value, item)
    queryStore.markStale('entry_updated')
  }

  function handleEntriesRemoved(ids: string[]) {
    for (const id of ids) {
      removeListItem(items.value, id)
    }
    queryStore.removeKnownIds(ids)
    queryStore.markStale('entries_removed')
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
