/**
 * 纯 Tauri IPC 封装层 — 无状态、无副作用。
 */
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import type {
  ClipboardEntriesQuery,
  ClipboardEntry,
} from '../types'

/** 统一查询：使用查询对象承载筛选条件、游标和分页参数。 */
export async function fetchEntries(query: ClipboardEntriesQuery): Promise<ClipboardEntry[]> {
  return invoke<ClipboardEntry[]>('get_entries', {
    query,
  })
}

export async function copyEntry(id: string): Promise<void> {
  return invoke('copy_entry', { id })
}

export async function deleteEntry(id: string): Promise<void> {
  return invoke('delete_entry', { id })
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
