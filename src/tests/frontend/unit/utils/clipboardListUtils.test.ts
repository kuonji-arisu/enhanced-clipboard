import { describe, expect, it } from 'vitest'
import {
  compareListItems,
  lastNonPinnedListItem,
  removeListItem,
  upsertListItem,
} from '../../../../stores/clipboardListUtils'
import { createTextListItem } from '../../support/factories'

describe('clipboardListUtils', () => {
  it('sorts pinned items first, then by timestamp and id descending', () => {
    const pinned = createTextListItem({ id: 'p', is_pinned: true, created_at: 10 })
    const newer = createTextListItem({ id: 'b', created_at: 20 })
    const older = createTextListItem({ id: 'a', created_at: 20 })

    expect(compareListItems(pinned, newer)).toBeLessThan(0)
    expect(compareListItems(newer, older)).toBeLessThan(0)
  })

  it('upserts and reorders items without duplicating ids', () => {
    const items = [
      createTextListItem({ id: 'b', created_at: 20 }),
      createTextListItem({ id: 'a', created_at: 10 }),
    ]

    upsertListItem(items, createTextListItem({ id: 'a', created_at: 30 }))

    expect(items.map((item) => item.id)).toEqual(['a', 'b'])
  })

  it('removes entries and returns the last non-pinned item', () => {
    const items = [
      createTextListItem({ id: 'p', is_pinned: true, created_at: 40 }),
      createTextListItem({ id: 'b', created_at: 30 }),
      createTextListItem({ id: 'a', created_at: 20 }),
    ]

    expect(lastNonPinnedListItem(items)?.id).toBe('a')

    removeListItem(items, 'b')

    expect(items.map((item) => item.id)).toEqual(['p', 'a'])
  })
})
