import { describe, expect, it } from 'vitest'
import fixture from '../../../../../tests/contracts/clipboard_page.json'
import type {
  ClipboardChanged,
  ClipboardContentType,
  ClipboardListPage,
  ClipboardPreview,
  ClipboardTextPreviewMode,
} from '../../../../types'

function contentType(value: string): ClipboardContentType {
  if (value === 'text' || value === 'image') return value
  throw new Error(`invalid clipboard content type: ${value}`)
}

function textPreviewMode(value: string): ClipboardTextPreviewMode {
  if (value === 'prefix' || value === 'search_snippet') return value
  throw new Error(`invalid clipboard text preview mode: ${value}`)
}

function preview(value: typeof fixture.page.items[number]['preview']): ClipboardPreview {
  if (
    value.kind === 'image'
    && 'src' in value
    && (typeof value.src === 'string' || value.src === null)
  ) {
    return { kind: 'image', src: value.src }
  }
  if (
    value.kind === 'text'
    && 'mode' in value
    && typeof value.mode === 'string'
    && 'text' in value
    && typeof value.text === 'string'
    && 'highlight_ranges' in value
    && Array.isArray(value.highlight_ranges)
  ) {
    return {
      kind: 'text',
      mode: textPreviewMode(value.mode),
      text: value.text,
      highlight_ranges: value.highlight_ranges.map(({ start, end }) => ({ start, end })),
    }
  }
  throw new Error(`invalid clipboard preview: ${JSON.stringify(value)}`)
}

describe('clipboard wire contract', () => {
  it('matches the shared page and invalidation shapes', () => {
    const event: ClipboardChanged = { revision: fixture.event.revision }
    const page: ClipboardListPage = {
      revision: fixture.page.revision,
      items: fixture.page.items.map((item) => ({
        id: item.id,
        content_type: contentType(item.content_type),
        tags: [...item.tags],
        created_at: item.created_at,
        is_pinned: item.is_pinned,
        source_app: item.source_app,
        preview: preview(item.preview),
        visible_until: item.visible_until,
      })),
      next_cursor: fixture.page.next_cursor
        ? { ...fixture.page.next_cursor }
        : null,
      pinned_count: fixture.page.pinned_count,
    }

    expect(event.revision).toBe(page.revision)
    expect(page.next_cursor).toEqual({ createdAt: 1699999999, id: 'ready-image' })
    expect(page.pinned_count).toBe(1)
    expect(page.items[0]?.preview).toMatchObject({
      kind: 'text',
      mode: 'search_snippet',
    })
    expect(page.items[1]?.preview).toMatchObject({ kind: 'image' })
  })
})
