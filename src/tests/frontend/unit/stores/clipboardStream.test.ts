import { describe, expect, it } from 'vitest'
import { useClipboardStreamStore } from '../../../../stores/clipboardStream'
import {
  createAppInfo,
  createAppSettings,
  createTextListItem,
} from '../../support/factories'
import { installTestPinia, primeAppInfoStore, primeSettingsStore } from '../../support/pinia'
import { setTauriInvokeHandler } from '../../support/tauri'

describe('clipboardStream store', () => {
  it('loads the stream by cursor pages and ignores unknown items past the loaded boundary', async () => {
    installTestPinia()
    primeAppInfoStore(createAppInfo({ page_size: 2 }))
    primeSettingsStore(createAppSettings({ expiry_seconds: 60 }))

    const pageOne = [
      createTextListItem({ id: 'pinned', is_pinned: true, created_at: 50 }),
      createTextListItem({ id: 'b', created_at: 40 }),
      createTextListItem({ id: 'a', created_at: 30 }),
    ]
    const pageTwo = [createTextListItem({ id: 'c', created_at: 20 })]

    setTauriInvokeHandler(async (command, args) => {
      if (command !== 'get_clipboard_list_items') {
        throw new Error(`unexpected command: ${command}`)
      }

      const query = args?.query as {
        cursor?: { id: string }
      }
      return query?.cursor ? pageTwo : pageOne
    })

    const store = useClipboardStreamStore()
    await store.loadInitial()

    const applied = store.applyStreamItemUpdated(createTextListItem({ id: 'too-old', created_at: 10 }))
    expect(applied).toBe(false)
    expect(store.items.map((item) => item.id)).not.toContain('too-old')

    await store.loadMore()

    expect(store.items.map((item) => item.id)).toEqual(['pinned', 'b', 'a', 'c'])
    expect(store.hasMore).toBe(false)
  })

  it('removes expired items using the saved retention window', () => {
    installTestPinia()
    primeAppInfoStore()
    primeSettingsStore(createAppSettings({ expiry_seconds: 20 }))

    const store = useClipboardStreamStore()
    store.items = [
      createTextListItem({ id: 'fresh', created_at: 95 }),
      createTextListItem({ id: 'stale', created_at: 10 }),
    ]

    expect(store.removeExpired(100)).toEqual(['stale'])
    expect(store.items.map((item) => item.id)).toEqual(['fresh'])
  })
})
