import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  clearAll,
  copyEntry,
  deleteEntry,
  fetchActiveDates as fetchActiveDatesApi,
  fetchEarliestMonth as fetchEarliestMonthApi,
  fetchEntries,
  resolveEntryForQuery as resolveEntryForQueryApi,
  togglePin as togglePinEntry,
} from '../composables/clipboardApi'
import { globalNow } from '../hooks/useNow'
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
import type {
  ClipboardEntriesQuery,
  ClipboardEntry,
  ClipboardQueryCursor,
} from '../types'

export const useClipboardStore = defineStore('clipboard', () => {
  // ── 数据源 ─────────────────────────────────────────────────────────────────
  // entries: 响应式数组，供模板渲染
  // map: 非响应式 Map，用于 O(1) 查找，与 entries 始终保持一致
  const entries = ref<ClipboardEntry[]>([])
  const map = new Map<string, ClipboardEntry>()

  const loading = ref(false)
  const loadingMore = ref(false)
  const hasMore = ref(true)
  const searchInput = ref('')
  const selectedDate = ref<string | null>(null)
  const searchCommandFilters = ref<EntrySearchCommandFilters>(createEntrySearchCommandFilters())
  const activeListQuery = ref<ClipboardEntriesQuery>({})
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

  const searchFilters = computed(() =>
    buildEntrySearchFilters(searchInput.value, searchCommandFilters.value),
  )

  function _buildFilterFields() {
    const text = searchFilters.value.text || undefined
    const entryType = searchFilters.value.entryType || undefined
    const tag = searchFilters.value.tag || undefined
    const date = selectedDate.value || undefined

    return { text, entryType, tag, date }
  }

  function _buildEntriesQuery(cursor?: ClipboardQueryCursor): ClipboardEntriesQuery {
    const { text, entryType, tag, date } = _buildFilterFields()
    return {
      text,
      entryType,
      tag,
      date,
      cursor,
      limit: _pageSize(),
    }
  }

  function _isDefaultListQuery(query: ClipboardEntriesQuery): boolean {
    return !query.text && !query.entryType && !query.tag && !query.date && !query.cursor
  }

  function _getLastNonPinnedEntry(): ClipboardEntry | undefined {
    const nonPinned = entries.value.filter((entry) => !entry.is_pinned)
    return nonPinned[nonPinned.length - 1]
  }

  function _pageSize(): number {
    return appInfoStore.requireAppInfo().page_size
  }

  function _isDefaultActiveListView(): boolean {
    return _isDefaultListQuery(activeListQuery.value)
  }

  function _captureActiveListQuery(query: ClipboardEntriesQuery): ClipboardEntriesQuery {
    return {
      text: query.text,
      entryType: query.entryType,
      tag: query.tag,
      date: query.date,
    }
  }

  function _buildActiveListQuery(cursor?: ClipboardQueryCursor): ClipboardEntriesQuery {
    return {
      text: activeListQuery.value.text,
      entryType: activeListQuery.value.entryType,
      tag: activeListQuery.value.tag,
      date: activeListQuery.value.date,
      cursor,
      limit: _pageSize(),
    }
  }

  async function _reconcileEntryForActiveQuery(id: string, revision: number) {
    const resolved = await resolveEntryForQueryApi(id, activeListQuery.value)
    if (revision !== _listRevision) return
    if (resolved) {
      _upsert(resolved)
      return
    }
    _remove(id)
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
      const pageSize = _pageSize()
      const listQuery = _buildEntriesQuery()
      const [result, earliest] = await Promise.all([
        fetchEntries(listQuery),
        fetchEarliestMonthApi(),
      ])

      if (revision !== _listRevision) return

      _replaceEntries(result)
      activeListQuery.value = _captureActiveListQuery(listQuery)
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
      const pageSize = _pageSize()
      const result = await fetchEntries(
        _buildActiveListQuery({
          createdAt: last.created_at,
          id: last.id,
        }),
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

  async function applySearch(date: string | null = selectedDate.value) {
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
    await togglePinEntry(id)
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
      if (_isDefaultActiveListView()) {
        _upsert(entry)
        return
      }

      const revision = _listRevision
      void _reconcileEntryForActiveQuery(entry.id, revision).catch((error) => {
        if (revision !== _listRevision) return
        console.error('[clipboard] failed to resolve added entry for active query:', error)
      })
    })

    const unlistenUpdated = await listen<ClipboardEntry>(
      'entry_updated',
      (event) => {
        const entry = event.payload
        if (_isDefaultActiveListView()) {
          if (!map.has(entry.id)) return
          _upsert(entry)
          return
        }

        const revision = _listRevision
        void _reconcileEntryForActiveQuery(entry.id, revision).catch((error) => {
          if (revision !== _listRevision) return
          console.error('[clipboard] failed to resolve updated entry for active query:', error)
        })
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
    searchInput, selectedDate, searchCommandFilters, searchFilters, earliestMonth, calendarRevision,
    get pinnedCount() { return entries.value.filter((e) => e.is_pinned).length },
    init, loadInitial, loadMore, setSearchInput, setSearchCommandFilter, clearSearchCommandFilter, applySearch, copy, remove, clear, togglePin, fetchActiveDates, refreshCalendarMeta,
  }
})

