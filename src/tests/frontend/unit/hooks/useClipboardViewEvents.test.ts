import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CLIPBOARD_QUERY_STALE_REASON } from '../../../../types'
import { useClipboardViewEvents } from '../../../../hooks/useClipboardViewEvents'
import { useCalendarMetaStore } from '../../../../stores/calendarMeta'
import { useClipboardActionsStore } from '../../../../stores/clipboardActions'
import { useClipboardQueryStore } from '../../../../stores/clipboardQuery'
import { useClipboardStreamStore } from '../../../../stores/clipboardStream'
import {
  createAppInfo,
  createImageListItem,
  createAppSettings,
  createTextListItem,
} from '../../support/factories'
import { installTestPinia, primeAppInfoStore, primeSettingsStore } from '../../support/pinia'
import { emitTauriEvent, setTauriInvokeHandler } from '../../support/tauri'
import { flushPromises } from '../../support/utils'

describe('useClipboardViewEvents', () => {
  let activeViewEvents: ReturnType<typeof useClipboardViewEvents> | null = null

  beforeEach(() => {
    installTestPinia()
    primeAppInfoStore(createAppInfo({ page_size: 2 }))
    primeSettingsStore(createAppSettings())
  })

  afterEach(() => {
    activeViewEvents?.stop()
    activeViewEvents = null
  })

  it('refreshes known snapshot items through the backend while applying stream updates', async () => {
    const querySnapshot = createTextListItem({
      id: 'entry-1',
      preview: {
        kind: 'text',
        mode: 'search_snippet',
        text: 'Query preview',
        highlight_ranges: [{ start: 0, end: 5 }],
      },
    })
    const refreshedSnapshot = createTextListItem({
      id: 'entry-1',
      preview: {
        kind: 'text',
        mode: 'search_snippet',
        text: 'Refreshed snapshot preview',
        highlight_ranges: [{ start: 0, end: 9 }],
      },
    })
    const streamPayload = createTextListItem({
      id: 'entry-1',
      preview: {
        kind: 'text',
        mode: 'prefix',
        text: 'Stream payload preview',
        highlight_ranges: [],
      },
    })

    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_clipboard_list_items') {
        const query = args?.query as { text?: string }
        if (query?.text) {
          return [querySnapshot]
        }
        return []
      }
      if (command === 'get_clipboard_list_item') {
        return refreshedSnapshot
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const queryStore = useClipboardQueryStore()
    const streamStore = useClipboardStreamStore()
    queryStore.setSearchInput('alpha')
    await queryStore.applySearch()

    activeViewEvents = useClipboardViewEvents()
    await activeViewEvents.start()
    await emitTauriEvent('clipboard_stream_item_updated', streamPayload)
    await flushPromises()

    expect(streamStore.items[0].preview).toEqual(streamPayload.preview)
    expect(queryStore.items[0].preview).toEqual(refreshedSnapshot.preview)
  })

  it('does not overwrite a known snapshot image item with the default stream payload', async () => {
    const querySnapshot = createImageListItem({
      id: 'image-1',
      thumbnail_path: 'C:/thumbnails/query.jpg',
    })
    const refreshedSnapshot = createImageListItem({
      id: 'image-1',
      thumbnail_path: 'C:/thumbnails/refreshed.jpg',
    })
    const streamPayload = createImageListItem({
      id: 'image-1',
      thumbnail_path: 'C:/thumbnails/stream.jpg',
    })

    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_clipboard_list_items') {
        const query = args?.query as { text?: string }
        if (query?.text) {
          return [querySnapshot]
        }
        return []
      }
      if (command === 'get_clipboard_list_item') {
        return refreshedSnapshot
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const queryStore = useClipboardQueryStore()
    const streamStore = useClipboardStreamStore()
    queryStore.setSearchInput('photo')
    await queryStore.applySearch()

    activeViewEvents = useClipboardViewEvents()
    await activeViewEvents.start()
    await emitTauriEvent('clipboard_stream_item_updated', streamPayload)
    await flushPromises()

    expect(streamStore.items[0].thumbnail_path).toBe('C:/thumbnails/stream.jpg')
    expect(queryStore.items[0].thumbnail_path).toBe('C:/thumbnails/refreshed.jpg')
  })

  it('reconciles settings-driven stale events through the real stream and calendar stores', async () => {
    const streamPayload = createTextListItem({ id: 'stream-entry' })
    let defaultLoadCount = 0

    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_clipboard_list_items') {
        const query = args?.query as { text?: string }
        if (query?.text) {
          return [createTextListItem({ id: 'snapshot-entry' })]
        }
        defaultLoadCount += 1
        return [streamPayload]
      }
      if (command === 'get_earliest_month') {
        return '2026-04'
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const queryStore = useClipboardQueryStore()
    const streamStore = useClipboardStreamStore()
    const calendarStore = useCalendarMetaStore()
    const loadInitial = vi.spyOn(streamStore, 'loadInitial')

    queryStore.setSearchInput('alpha')
    await queryStore.applySearch()

    activeViewEvents = useClipboardViewEvents()
    await activeViewEvents.start()
    await emitTauriEvent(
      'clipboard_query_results_stale',
      CLIPBOARD_QUERY_STALE_REASON.SETTINGS_OR_STARTUP,
    )
    await flushPromises()
    await flushPromises()

    expect(queryStore.stale).toBe(true)
    expect(queryStore.staleReason).toBe(CLIPBOARD_QUERY_STALE_REASON.SETTINGS_OR_STARTUP)
    expect(defaultLoadCount).toBe(1)
    expect(loadInitial).toHaveBeenCalledOnce()
    expect(calendarStore.earliestMonth).toBe('2026-04')
    expect(calendarStore.calendarRevision).toBe(1)
  })

  it('applies stream additions to the default list and bumps calendar metadata', async () => {
    const streamStore = useClipboardStreamStore()
    const calendarStore = useCalendarMetaStore()
    const added = createTextListItem({ id: 'new-stream-item' })

    setTauriInvokeHandler(async (command) => {
      throw new Error(`unexpected command: ${command}`)
    })

    activeViewEvents = useClipboardViewEvents()
    await activeViewEvents.start()
    await emitTauriEvent('clipboard_stream_item_added', added)
    await flushPromises()

    expect(streamStore.items.map((item) => item.id)).toEqual(['new-stream-item'])
    expect(calendarStore.calendarRevision).toBe(1)
  })

  it('stops listening to clipboard events after stop', async () => {
    const streamStore = useClipboardStreamStore()

    setTauriInvokeHandler(async (command) => {
      throw new Error(`unexpected command: ${command}`)
    })

    activeViewEvents = useClipboardViewEvents()
    await activeViewEvents.start()
    await emitTauriEvent('clipboard_stream_item_added', createTextListItem({ id: 'before-stop' }))
    await flushPromises()

    activeViewEvents.stop()
    await emitTauriEvent('clipboard_stream_item_added', createTextListItem({ id: 'after-stop' }))
    await flushPromises()

    expect(streamStore.items.map((item) => item.id)).toEqual(['before-stop'])
  })

  it('removes ids from stream and snapshot views and refreshes calendar metadata', async () => {
    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_clipboard_list_items') {
        const query = args?.query as { text?: string }
        if (query?.text) {
          return [createTextListItem({ id: 'shared-id' })]
        }
        return [createTextListItem({ id: 'shared-id' }), createTextListItem({ id: 'stream-only' })]
      }
      if (command === 'get_earliest_month') {
        return '2026-04'
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const queryStore = useClipboardQueryStore()
    const streamStore = useClipboardStreamStore()
    const calendarStore = useCalendarMetaStore()
    const refreshCalendarMeta = vi.spyOn(calendarStore, 'refreshCalendarMeta')

    await streamStore.loadInitial()
    queryStore.setSearchInput('alpha')
    await queryStore.applySearch()

    activeViewEvents = useClipboardViewEvents()
    await activeViewEvents.start()
    await emitTauriEvent('entries_removed', ['shared-id'])
    await flushPromises()

    expect(streamStore.items.map((item) => item.id)).toEqual(['stream-only'])
    expect(queryStore.items).toEqual([])
    expect(refreshCalendarMeta).toHaveBeenCalledOnce()
  })

  it('removes broken image entries from the stream after backend acknowledgement and removal events', async () => {
    const imageItem = createImageListItem({ id: 'broken-image' })

    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_clipboard_list_items') {
        return [imageItem]
      }
      if (command === 'report_image_load_failed') {
        expect(args).toEqual({ id: 'broken-image' })
        return true
      }
      if (command === 'get_earliest_month') {
        return '2026-04'
      }
      throw new Error(`unexpected command: ${command}`)
    })

    const actionsStore = useClipboardActionsStore()
    const streamStore = useClipboardStreamStore()

    await streamStore.loadInitial()
    activeViewEvents = useClipboardViewEvents()
    await activeViewEvents.start()

    await expect(actionsStore.handleImageLoadFailed('broken-image')).resolves.toBe(true)
    await emitTauriEvent('entries_removed', ['broken-image'])
    await flushPromises()

    expect(streamStore.items).toEqual([])
  })
})
