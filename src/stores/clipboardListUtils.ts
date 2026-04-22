import type { ClipboardListItem } from '../types'

export function compareListItems(a: ClipboardListItem, b: ClipboardListItem): number {
  if (a.is_pinned !== b.is_pinned) return a.is_pinned ? -1 : 1
  if (a.created_at !== b.created_at) return a.created_at > b.created_at ? -1 : 1
  if (a.id !== b.id) return a.id > b.id ? -1 : 1
  return 0
}

export function findListItemInsertIndex(
  items: ClipboardListItem[],
  item: ClipboardListItem,
): number {
  const idx = items.findIndex((current) => compareListItems(item, current) < 0)
  return idx === -1 ? items.length : idx
}

export function upsertListItem(items: ClipboardListItem[], item: ClipboardListItem): void {
  const idx = items.findIndex((current) => current.id === item.id)
  if (idx !== -1) items.splice(idx, 1)
  items.splice(findListItemInsertIndex(items, item), 0, item)
}

export function removeListItem(items: ClipboardListItem[], id: string): void {
  const idx = items.findIndex((item) => item.id === id)
  if (idx !== -1) items.splice(idx, 1)
}

export function lastNonPinnedListItem(
  items: ClipboardListItem[],
): ClipboardListItem | undefined {
  const nonPinned = items.filter((item) => !item.is_pinned)
  return nonPinned[nonPinned.length - 1]
}
