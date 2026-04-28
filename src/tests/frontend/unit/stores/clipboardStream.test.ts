import { describe, expect, it } from 'vitest'
import { useClipboardStreamStore } from '../../../../stores/clipboardStream'
import {
  createAppInfo,
  createAppSettings,
  createImageListItem,
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

  it('replaces a pending image with the ready stream update', () => {
    installTestPinia()
    primeAppInfoStore()
    primeSettingsStore()

    const store = useClipboardStreamStore()
    store.applyStreamItemAdded(createImageListItem({
      id: 'image',
      preview: { kind: 'image', mode: 'pending' },
      image_path: null,
      thumbnail_path: null,
    }))
    store.applyStreamItemUpdated(createImageListItem({
      id: 'image',
      preview: { kind: 'image', mode: 'ready' },
      image_path: 'C:/images/image.png',
      thumbnail_path: 'C:/thumbnails/image.jpg',
    }))

    expect(store.items).toHaveLength(1)
    expect(store.items[0].preview).toEqual({ kind: 'image', mode: 'ready' })
    expect(store.items[0].thumbnail_path).toBe('C:/thumbnails/image.jpg')
  })

  it('releases loaded items and ignores an in-flight initial load result', async () => {
    installTestPinia()
    primeAppInfoStore(createAppInfo({ page_size: 2 }))
    primeSettingsStore()

    let resolveLoad: (items: ReturnType<typeof createTextListItem>[]) => void = () => {}
    const pendingLoad = new Promise<ReturnType<typeof createTextListItem>[]>((resolve) => {
      resolveLoad = resolve
    })

    setTauriInvokeHandler(async (command) => {
      if (command === 'get_clipboard_list_items') {
        return pendingLoad
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const store = useClipboardStreamStore()
    const load = store.loadInitial()
    expect(store.loading).toBe(true)

    store.releaseLoadedItems()
    expect(store.items).toEqual([])
    expect(store.loading).toBe(false)
    expect(store.loadingMore).toBe(false)
    expect(store.hasMore).toBe(true)

    resolveLoad([createTextListItem({ id: 'late-result' })])
    await load

    expect(store.items).toEqual([])
    expect(store.loading).toBe(false)
    expect(store.hasMore).toBe(true)
  })
})
