import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { fetchClipboardListItems } from '../composables/clipboardApi'
import { globalNow } from '../hooks/useNow'
import { useAppInfoStore } from './appInfo'
import { useSettingsStore } from './settings'
import {
  compareListItems,
  lastNonPinnedListItem,
  removeListItem,
  upsertListItem,
} from './clipboardListUtils'
import type { ClipboardListItem } from '../types'

// Default history stream state. This is the main list source of truth and is
// allowed to apply view-facing stream item changes incrementally. Cross-view
// side effects such as snapshot stale signaling live in the view coordinator.
export const useClipboardStreamStore = defineStore('clipboardStream', () => {
  const items = ref<ClipboardListItem[]>([])
  const loading = ref(false)
  const loadingMore = ref(false)
  const hasMore = ref(true)

  let listRevision = 0

  const appInfoStore = useAppInfoStore()
  const settingsStore = useSettingsStore()

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

  function applyStreamItemAdded(item: ClipboardListItem) {
    upsertListItem(items.value, item)
  }

  function shouldApplyUnknownStreamUpdate(item: ClipboardListItem): boolean {
    if (item.is_pinned || !hasMore.value) return true

    const lastLoadedNormal = lastNonPinnedListItem(items.value)
    if (!lastLoadedNormal) return items.value.length === 0

    return compareListItems(item, lastLoadedNormal) < 0
  }

  function applyStreamItemUpdated(item: ClipboardListItem): boolean {
    const alreadyLoaded = items.value.some((current) => current.id === item.id)
    if (!alreadyLoaded && !shouldApplyUnknownStreamUpdate(item)) return false
    upsertListItem(items.value, item)
    return true
  }

  function removeIds(ids: string[]) {
    for (const id of ids) {
      removeListItem(items.value, id)
    }
  }

  function markCleared() {
    items.value = []
    hasMore.value = false
  }

  return {
    items,
    loading,
    loadingMore,
    hasMore,
    pinnedCount,
    loadInitial,
    loadMore,
    applyStreamItemAdded,
    applyStreamItemUpdated,
    removeIds,
    markCleared,
  }
})
