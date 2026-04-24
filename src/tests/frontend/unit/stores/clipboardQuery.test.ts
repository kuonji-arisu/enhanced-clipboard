import { describe, expect, it } from 'vitest'
import { CLIPBOARD_QUERY_STALE_REASON } from '../../../../types'
import { useClipboardQueryStore } from '../../../../stores/clipboardQuery'
import {
  createAppInfo,
  createAppSettings,
  createTextListItem,
} from '../../support/factories'
import { installTestPinia, primeAppInfoStore, primeSettingsStore } from '../../support/pinia'
import { setTauriInvokeHandler } from '../../support/tauri'

describe('clipboardQuery store', () => {
  it('loads snapshot results, marks TTL stale, and refreshes known items', async () => {
    installTestPinia()
    primeAppInfoStore(createAppInfo({ page_size: 2 }))
    primeSettingsStore(createAppSettings({ expiry_seconds: 20 }))

    const initialSnapshot = [
      createTextListItem({
        id: 'alpha-2',
        created_at: 90,
        preview: {
          kind: 'text',
          mode: 'search_snippet',
          text: 'Alpha two',
          highlight_ranges: [{ start: 0, end: 5 }],
        },
      }),
      createTextListItem({
        id: 'alpha-1',
        created_at: 10,
        preview: {
          kind: 'text',
          mode: 'search_snippet',
          text: 'Alpha one',
          highlight_ranges: [{ start: 0, end: 5 }],
        },
      }),
    ]
    let refreshedItem = createTextListItem({
      id: 'alpha-2',
      created_at: 90,
      preview: {
        kind: 'text',
        mode: 'search_snippet',
        text: 'Alpha updated',
        highlight_ranges: [{ start: 0, end: 5 }],
      },
    })

    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_clipboard_list_items') {
        const query = args?.query as { text?: string }
        return query?.text === 'alpha' ? initialSnapshot : []
      }
      if (command === 'get_clipboard_list_item') {
        return refreshedItem
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const store = useClipboardQueryStore()
    store.setSearchInput('alpha')
    await store.applySearch()

    expect(store.isSnapshotView).toBe(true)
    expect(store.activeQuery).toEqual({
      text: 'alpha',
      entryType: undefined,
      tag: undefined,
      date: undefined,
    })

    const removedIds = store.removeExpired(100)
    expect(removedIds).toEqual(['alpha-1'])
    expect(store.stale).toBe(true)
    expect(store.staleReason).toBe(CLIPBOARD_QUERY_STALE_REASON.TTL_EXPIRED)

    await store.applySearch()
    await store.refreshKnownItem('alpha-2')
    expect(store.items[0].preview.kind).toBe('text')
    if (store.items[0].preview.kind === 'text') {
      expect(store.items[0].preview.text).toBe('Alpha updated')
    }

    refreshedItem = null as never
    await store.refreshKnownItem('alpha-2')
    expect(store.items.map((item) => item.id)).toEqual(['alpha-1'])
  })

  it('resets snapshot state when the active query returns to the default view', async () => {
    installTestPinia()
    primeAppInfoStore()
    primeSettingsStore()

    setTauriInvokeHandler(async (command) => {
      if (command === 'get_clipboard_list_items') {
        return [createTextListItem()]
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const store = useClipboardQueryStore()
    store.setSearchInput('alpha')
    await store.applySearch()

    await store.clearSearch()

    expect(store.isDefaultView).toBe(true)
    expect(store.items).toEqual([])
    expect(store.hasMore).toBe(false)
    expect(store.stale).toBe(false)
  })
})
