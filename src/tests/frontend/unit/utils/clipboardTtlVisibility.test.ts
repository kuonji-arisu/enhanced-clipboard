import { describe, expect, it } from 'vitest'
import {
  isTtlVisible,
  removeExpiredListItems,
} from '../../../../stores/clipboardTtlVisibility'
import { createTextListItem } from '../../support/factories'

describe('clipboardTtlVisibility', () => {
  it('keeps pinned items visible regardless of expiry', () => {
    const item = createTextListItem({ is_pinned: true, created_at: 10 })
    expect(isTtlVisible(item, 100, 1)).toBe(true)
  })

  it('removes expired non-pinned items in place', () => {
    const items = [
      createTextListItem({ id: 'fresh', created_at: 95 }),
      createTextListItem({ id: 'stale', created_at: 10 }),
      createTextListItem({ id: 'pinned', created_at: 1, is_pinned: true }),
    ]

    expect(removeExpiredListItems(items, 100, 20)).toEqual(['stale'])
    expect(items.map((item) => item.id)).toEqual(['fresh', 'pinned'])
  })
})
