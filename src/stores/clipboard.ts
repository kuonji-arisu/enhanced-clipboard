import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  clearAll,
  copyEntry,
  deleteEntry,
  fetchActiveDates as fetchActiveDatesApi,
  fetchEarliestMonth as fetchEarliestMonthApi,
  fetchEntries,
  togglePin as togglePinEntry,
} from '../composables/clipboardApi'
import { globalNow } from '../hooks/useNow'
import { useAppInfoStore } from './appInfo'
import { useSettingsStore } from './settings'
import type { ClipboardEntry } from '../types'

export const useClipboardStore = defineStore('clipboard', () => {
  // ── 数据源 ─────────────────────────────────────────────────────────────────
  // entries: 响应式数组，供模板渲染
  // map: 非响应式 Map，用于 O(1) 查找，与 entries 始终保持一致
  const entries = ref<ClipboardEntry[]>([])
  const map = new Map<string, ClipboardEntry>()

  const loading = ref(false)
  const loadingMore = ref(false)
  const hasMore = ref(true)
  const searchQuery = ref('')
  const selectedDate = ref<string | null>(null)
  const earliestMonth = ref<string | null>(null)
  const calendarRevision = ref(0)

  let _unlisten: UnlistenFn | null = null
  let _listRevision = 0

  const appInfoStore = useAppInfoStore()
  const settingsStore = useSettingsStore()

  // ── 私有辅助函数 ──────────────────────────────────────────────────────────

  function _compareEntries(a: ClipboardEntry, b: ClipboardEntry): number {
    if (a.is_pinned !== b.is_pinned) return a.is_pinned ? -1 : 1
    if (a.created_at !== b.created_at) return a.created_at > b.created_at ? -1 : 1
    if (a.id !== b.id) return a.id > b.id ? -1 : 1
    return 0
  }

  function _findInsertIndex(arr: ClipboardEntry[], entry: ClipboardEntry): number {
    const idx = arr.findIndex((current) => _compareEntries(entry, current) < 0)
    return idx === -1 ? arr.length : idx
  }

  /** 将条目插入或替换（全量替换以确保响应式触发），维护统一排序。 */
  function _upsert(entry: ClipboardEntry): void {
    const idx = entries.value.findIndex((e) => e.id === entry.id)
    if (idx !== -1) entries.value.splice(idx, 1)

    const insertAt = _findInsertIndex(entries.value, entry)
    entries.value.splice(insertAt, 0, entry)
    map.set(entry.id, entry)
  }

  /** 移除单个条目（幂等） */
  function _remove(id: string): void {
    if (!map.has(id)) return
    const idx = entries.value.findIndex((e) => e.id === id)
    if (idx !== -1) entries.value.splice(idx, 1)
    map.delete(id)
  }

  function _replaceEntries(nextEntries: ClipboardEntry[]): void {
    entries.value = []
    map.clear()
    for (const entry of nextEntries) {
      entries.value.push(entry)
      map.set(entry.id, entry)
    }
  }

  function _getFilterArgs() {
    return {
      query: searchQuery.value || undefined,
      date: selectedDate.value || undefined,
    }
  }

  function _getLastNonPinnedEntry(): ClipboardEntry | undefined {
    const nonPinned = entries.value.filter((entry) => !entry.is_pinned)
    return nonPinned[nonPinned.length - 1]
  }

  function _pageSize(): number {
    return appInfoStore.requireAppInfo().page_size
  }

  function _matchesSelectedDate(entry: ClipboardEntry): boolean {
    if (!selectedDate.value) return true
    const date = new Date(entry.created_at * 1000)
    const ymd = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
    return ymd === selectedDate.value
  }

  function _shouldIncludeRealtimeEntry(entry: ClipboardEntry): boolean {
    if (searchQuery.value) return false
    return _matchesSelectedDate(entry)
  }

  /** 图片写入完成后更新 image_path 和 thumbnail_path（替换对象以确保响应式触发） */
  function _patchImagePaths(
    id: string,
    imagePath: string,
    thumbnailPath: string | null,
  ): void {
    const entry = map.get(id)
    if (!entry) return
    const updated = { ...entry, image_path: imagePath, thumbnail_path: thumbnailPath }
    map.set(id, updated)
    const idx = entries.value.findIndex((e) => e.id === id)
    if (idx !== -1) entries.value.splice(idx, 1, updated)
  }

  /** 移除所有过期非置顶条目 */
  function _removeExpired(now: number): void {
    const ttl = settingsStore.savedSettings.expiry_seconds
    if (ttl <= 0) return
    const cutoff = now - ttl
    const expired = entries.value.filter((e) => !e.is_pinned && e.created_at < cutoff)
    for (const e of expired) _remove(e.id)
  }

  // ── 每秒清理过期条目 ──────────────────────────────────────────────────────
  watch(globalNow, (now) => {
    _removeExpired(now)
  })

  // ── 加载 ───────────────────────────────────────────────────────────────────
  async function loadInitial() {
    const revision = ++_listRevision
    loading.value = true
    loadingMore.value = false
    hasMore.value = true
    try {
      const { query, date } = _getFilterArgs()
      const pageSize = _pageSize()
      const [result, earliest] = await Promise.all([
        fetchEntries(query, date, undefined, undefined, pageSize),
        fetchEarliestMonthApi(),
      ])

      if (revision !== _listRevision) return

      _replaceEntries(result)
      const normalCount = result.filter((e) => !e.is_pinned).length
      hasMore.value = normalCount === pageSize
      earliestMonth.value = earliest
    } finally {
      if (revision === _listRevision) loading.value = false
    }
  }

  async function loadMore() {
    if (loadingMore.value || !hasMore.value) return
    const revision = _listRevision
    loadingMore.value = true
    try {
      const last = _getLastNonPinnedEntry()
      if (!last) {
        hasMore.value = false
        return
      }
      const { query, date } = _getFilterArgs()
      const pageSize = _pageSize()
      const result = await fetchEntries(
        query,
        date,
        last.created_at,
        last.id,
        pageSize,
      )
      if (revision !== _listRevision) return
      for (const entry of result) {
        _upsert(entry)
      }
      hasMore.value = result.length === pageSize
    } finally {
      loadingMore.value = false
    }
  }

  async function setFilter(query: string, date: string | null) {
    searchQuery.value = query
    selectedDate.value = date
    await loadInitial()
  }

  async function refreshEarliestMonth() {
    earliestMonth.value = await fetchEarliestMonthApi()
  }

  async function refreshCalendarMeta() {
    await refreshEarliestMonth()
    calendarRevision.value += 1
  }

  function notifyCalendarDatesChanged() {
    calendarRevision.value += 1
  }

  // ── 用户操作 ───────────────────────────────────────────────────────────────
  async function copy(id: string) {
    await copyEntry(id)
  }

  async function remove(id: string) {
    await deleteEntry(id)
  }

  async function clear() {
    await clearAll()
    hasMore.value = false
  }

  async function togglePin(id: string) {
    const newState = await togglePinEntry(id)
    const entry = map.get(id)
    if (entry) {
      _remove(id)
      _upsert({ ...entry, is_pinned: newState })
    }
  }

  async function fetchActiveDates(yearMonth: string) {
    return fetchActiveDatesApi(yearMonth)
  }

  // ── 事件监听（增量更新） ───────────────────────────────────────────────────
  async function startListening() {
    if (_unlisten) return

    const unlistenAdded = await listen<ClipboardEntry>('entry_added', (event) => {
      const entry = event.payload
      notifyCalendarDatesChanged()
      if (!_shouldIncludeRealtimeEntry(entry)) return
      _upsert(entry)
    })

    const unlistenUpdated = await listen<{
      id: string
      image_path: string
      thumbnail_path: string | null
    }>(
      'entry_updated',
      (event) => {
        const { id, image_path, thumbnail_path } = event.payload
        _patchImagePaths(id, image_path, thumbnail_path)
      },
    )

    const unlistenRemoved = await listen<string[]>('entries_removed', (event) => {
      for (const id of event.payload) {
        _remove(id)
      }
      void refreshCalendarMeta().catch((error) => {
        console.error('[clipboard] failed to refresh calendar metadata:', error)
      })
    })

    _unlisten = () => {
      unlistenAdded()
      unlistenUpdated()
      unlistenRemoved()
    }
  }

  /** 幂等初始化：首次调用加载数据 + 开始监听，后续调用为 no-op */
  async function init() {
    await startListening()
    if (entries.value.length === 0) {
      await loadInitial()
    } else {
      await refreshEarliestMonth()
    }
  }

  return {
    entries, loading, loadingMore, hasMore,
    searchQuery, selectedDate, earliestMonth, calendarRevision,
    get pinnedCount() { return entries.value.filter((e) => e.is_pinned).length },
    init, loadInitial, loadMore, setFilter, copy, remove, clear, togglePin, fetchActiveDates, refreshCalendarMeta,
  }
})

