/**
 * 纯 Tauri IPC 封装层 — 无状态、无副作用。
 */
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  ClipboardEntriesQuery,
  ClipboardListItem,
  ClipboardQueryStaleReason,
} from '../types'

const EVENT_STREAM_ITEM_ADDED = 'clipboard_stream_item_added'
const EVENT_STREAM_ITEM_UPDATED = 'clipboard_stream_item_updated'
const EVENT_ENTRIES_REMOVED = 'entries_removed'
const EVENT_QUERY_RESULTS_STALE = 'clipboard_query_results_stale'

/** 统一查询：使用查询对象承载筛选条件、游标和分页参数。 */
export async function fetchClipboardListItems(
  query: ClipboardEntriesQuery,
): Promise<ClipboardListItem[]> {
  return invoke<ClipboardListItem[]>('get_clipboard_list_items', {
    query,
  })
}

export async function fetchClipboardListItem(
  id: string,
  query: ClipboardEntriesQuery,
): Promise<ClipboardListItem | null> {
  return invoke<ClipboardListItem | null>('get_clipboard_list_item', {
    id,
    query,
  })
}

export interface ClipboardEventHandlers {
  onStreamItemAdded: (item: ClipboardListItem) => void
  onStreamItemUpdated: (item: ClipboardListItem) => void
  onEntriesRemoved: (ids: string[]) => void
  onQueryResultsStale: (reason: ClipboardQueryStaleReason) => void
}

export async function listenClipboardEvents(
  handlers: ClipboardEventHandlers,
): Promise<UnlistenFn> {
  const unlistenAdded = await listen<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED, (event) => {
    handlers.onStreamItemAdded(event.payload)
  })
  const unlistenUpdated = await listen<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED, (event) => {
    handlers.onStreamItemUpdated(event.payload)
  })
  const unlistenRemoved = await listen<string[]>(EVENT_ENTRIES_REMOVED, (event) => {
    handlers.onEntriesRemoved(event.payload)
  })
  const unlistenStale = await listen<ClipboardQueryStaleReason>(
    EVENT_QUERY_RESULTS_STALE,
    (event) => {
      handlers.onQueryResultsStale(event.payload)
    },
  )

  return () => {
    unlistenAdded()
    unlistenUpdated()
    unlistenRemoved()
    unlistenStale()
  }
}

export async function copyEntry(id: string): Promise<void> {
  return invoke('copy_entry', { id })
}

export async function deleteEntry(id: string): Promise<void> {
  return invoke('delete_entry', { id })
}

export async function reportImageLoadFailed(id: string): Promise<void> {
  return invoke('report_image_load_failed', { id })
}

export async function clearAll(): Promise<void> {
  return invoke('clear_all')
}

export async function togglePin(id: string): Promise<void> {
  return invoke('toggle_pin', { id })
}

export async function fetchActiveDates(yearMonth: string): Promise<string[]> {
  return invoke<string[]>('get_active_dates', { yearMonth })
}

export async function fetchEarliestMonth(): Promise<string | null> {
  return invoke<string | null>('get_earliest_month')
}

export function getImageSrc(filePath: string): string {
  return convertFileSrc(filePath)
}
