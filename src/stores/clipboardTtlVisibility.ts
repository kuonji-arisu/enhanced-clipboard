import type { ClipboardListItem } from '../types'

export function isTtlVisible(
  item: ClipboardListItem,
  now: number,
  expirySeconds: number,
): boolean {
  if (item.is_pinned || expirySeconds <= 0) return true
  return item.created_at >= now - expirySeconds
}

export function removeExpiredListItems(
  items: ClipboardListItem[],
  now: number,
  expirySeconds: number,
): string[] {
  if (expirySeconds <= 0) return []

  const removedIds: string[] = []
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index]
    if (isTtlVisible(item, now, expirySeconds)) continue
    removedIds.push(item.id)
    items.splice(index, 1)
  }

  return removedIds
}
