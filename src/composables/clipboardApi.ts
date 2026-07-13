/**
 * 纯 Tauri IPC 封装层 — 无状态、无副作用。
 */
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  ClipboardChanged,
  ClipboardEntriesQuery,
  ClipboardListPage,
  ImagePreviewRepairOutcome,
} from '../types'

export type ClipboardChangedUnlisten = UnlistenFn

const EVENT_CLIPBOARD_CHANGED = 'clipboard_changed'

/** 统一查询：使用查询对象承载筛选条件、游标和分页参数。 */
export async function fetchClipboardListPage(
  query: ClipboardEntriesQuery,
): Promise<ClipboardListPage> {
  return invoke<ClipboardListPage>('get_clipboard_list_items', {
    query,
  })
}

export async function listenClipboardChanged(
  handler: (payload: ClipboardChanged) => void | Promise<void>,
): Promise<ClipboardChangedUnlisten> {
  return listen<ClipboardChanged>(EVENT_CLIPBOARD_CHANGED, (event) => handler(event.payload))
}

export async function copyEntry(id: string): Promise<void> {
  return invoke('copy_entry', { id })
}

export async function deleteEntry(id: string): Promise<void> {
  return invoke('delete_entry', { id })
}

export async function reportImageLoadFailed(id: string): Promise<ImagePreviewRepairOutcome> {
  return invoke<ImagePreviewRepairOutcome>('report_image_load_failed', { id })
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
