import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useClipboardPageLifecycle } from '../../../../hooks/useClipboardPageLifecycle'
import { useCalendarMetaStore } from '../../../../stores/calendarMeta'
import { useClipboardQueryStore } from '../../../../stores/clipboardQuery'
import { useClipboardStreamStore } from '../../../../stores/clipboardStream'
import {
  createAppInfo,
  createAppSettings,
  createTextListItem,
} from '../../support/factories'
import { installTestPinia, primeAppInfoStore, primeSettingsStore } from '../../support/pinia'
import { emitTauriEvent, setTauriInvokeHandler } from '../../support/tauri'
import { flushPromises } from '../../support/utils'

describe('useClipboardPageLifecycle', () => {
  let lifecycle: ReturnType<typeof useClipboardPageLifecycle> | null = null

  beforeEach(() => {
    installTestPinia()
    primeAppInfoStore(createAppInfo({ page_size: 2 }))
    primeSettingsStore(createAppSettings())
  })

  afterEach(() => {
    lifecycle?.releaseViewCache()
    lifecycle = null
  })

  it('stops view events and releases stream and query caches', async () => {
    const streamStore = useClipboardStreamStore()
    const queryStore = useClipboardQueryStore()

    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_clipboard_list_items') {
        const query = args?.query as { text?: string } | undefined
        if (query?.text) {
          return [createTextListItem({ id: 'query-entry' })]
        }
        return [createTextListItem({ id: 'stream-entry' })]
      }
      if (command === 'get_earliest_month') {
        return '2026-04'
      }
      throw new Error(`unexpected command: ${command}`)
    })

    await streamStore.loadInitial()
    queryStore.setSearchInput('alpha')
    await queryStore.applySearch()

    lifecycle = useClipboardPageLifecycle()
    await lifecycle.initStreamView()
    lifecycle.releaseViewCache()

    await emitTauriEvent('clipboard_stream_item_added', createTextListItem({ id: 'after-release' }))
    await flushPromises()

    expect(streamStore.items).toEqual([])
    expect(streamStore.loading).toBe(false)
    expect(streamStore.loadingMore).toBe(false)
    expect(streamStore.hasMore).toBe(true)
    expect(queryStore.items).toEqual([])
    expect(queryStore.loading).toBe(false)
    expect(queryStore.loadingMore).toBe(false)
    expect(queryStore.hasMore).toBe(true)
    expect(queryStore.searchInput).toBe('alpha')
  })

  it('resumes the default view through search state and stream bootstrap', async () => {
    const streamStore = useClipboardStreamStore()
    const queryStore = useClipboardQueryStore()
    const calendarStore = useCalendarMetaStore()
    const loadInitial = vi.spyOn(streamStore, 'loadInitial')

    setTauriInvokeHandler(async (command) => {
      if (command === 'get_clipboard_list_items') {
        return [createTextListItem({ id: 'stream-entry' })]
      }
      if (command === 'get_earliest_month') {
        return '2026-04'
      }
      throw new Error(`unexpected command: ${command}`)
    })

    lifecycle = useClipboardPageLifecycle()
    await lifecycle.resumeView()

    expect(queryStore.isDefaultView).toBe(true)
    expect(loadInitial).toHaveBeenCalledOnce()
    expect(streamStore.items.map((item) => item.id)).toEqual(['stream-entry'])
    expect(calendarStore.earliestMonth).toBe('2026-04')
  })

  it('resumes a filtered view from current UI search controls without clearing context', async () => {
    const streamStore = useClipboardStreamStore()
    const queryStore = useClipboardQueryStore()
    const loadInitial = vi.spyOn(streamStore, 'loadInitial')

    setTauriInvokeHandler(async (command, args) => {
      if (command === 'get_clipboard_list_items') {
        const query = args?.query as { text?: string, entryType?: string, date?: string }
        expect(query.text).toBe('alpha')
        expect(query.entryType).toBe('text')
        expect(query.date).toBe('2026-04-24')
        return [createTextListItem({ id: 'filtered-entry' })]
      }
      throw new Error(`unexpected command: ${command}`)
    })

    queryStore.setSearchInput('alpha')
    queryStore.setSearchCommandFilter('type', 'text')
    queryStore.selectedDate = '2026-04-24'

    lifecycle = useClipboardPageLifecycle()
    await lifecycle.resumeView()

    expect(loadInitial).not.toHaveBeenCalled()
    expect(queryStore.items.map((item) => item.id)).toEqual(['filtered-entry'])
    expect(queryStore.searchInput).toBe('alpha')
    expect(queryStore.selectedDate).toBe('2026-04-24')
    expect(queryStore.searchCommandFilters).toEqual({ type: 'text', tag: null })
    expect(queryStore.activeQuery).toEqual({
      text: 'alpha',
      entryType: 'text',
      tag: undefined,
      date: '2026-04-24',
    })
  })
})
