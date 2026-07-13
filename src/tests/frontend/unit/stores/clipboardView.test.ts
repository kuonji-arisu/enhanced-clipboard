import { beforeEach, describe, expect, it } from 'vitest'
import { globalNow } from '../../../../hooks/useNow'
import { useClipboardViewStore } from '../../../../stores/clipboardView'
import type { ClipboardListPage } from '../../../../types'
import { createAppInfo, createTextListItem } from '../../support/factories'
import { installTestPinia, primeAppInfoStore } from '../../support/pinia'
import {
  emitTauriEvent,
  setTauriInvokeHandler,
  tauriListenMock,
} from '../../support/tauri'
import { flushPromises } from '../../support/utils'

function page(overrides: Partial<ClipboardListPage> = {}): ClipboardListPage {
  return {
    revision: 1,
    items: [createTextListItem()],
    next_cursor: null,
    pinned_count: 0,
    ...overrides,
  }
}

function deferred<T>() {
  let resolve: (value: T) => void = () => {}
  let reject: (reason?: unknown) => void = () => {}
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

describe('clipboard view store', () => {
  beforeEach(() => {
    installTestPinia()
    primeAppInfoStore(createAppInfo({ page_size: 2 }))
    globalNow.value = 1_700_000_000
  })

  it('loads the page envelope and hides expired items without mutating the cache', async () => {
    setTauriInvokeHandler((command) => {
      if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
      return page({
        revision: 4,
        pinned_count: 7,
        items: [
          createTextListItem({ id: 'expired', visible_until: globalNow.value }),
          createTextListItem({ id: 'visible', visible_until: globalNow.value + 10 }),
          createTextListItem({ id: 'pinned', is_pinned: true, visible_until: null }),
        ],
      })
    })

    const store = useClipboardViewStore()
    await store.start()

    expect(store.loadedRevision).toBe(4)
    expect(store.pinnedCount).toBe(7)
    expect(store.items.map(({ id }) => id)).toEqual(['expired', 'visible', 'pinned'])
    expect(store.visibleItems.map(({ id }) => id)).toEqual(['visible', 'pinned'])

    globalNow.value += 11
    expect(store.visibleItems.map(({ id }) => id)).toEqual(['pinned'])
    expect(store.items).toHaveLength(3)
  })

  it('collapses invalidations received during refresh into one follow-up read', async () => {
    const inFlight = deferred<ClipboardListPage>()
    let listCalls = 0
    setTauriInvokeHandler((command) => {
      if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
      listCalls += 1
      if (listCalls === 1) return page({ revision: 1 })
      if (listCalls === 2) return inFlight.promise
      return page({
        revision: 3,
        items: [createTextListItem({ id: 'latest' })],
      })
    })

    const store = useClipboardViewStore()
    await store.start()
    await emitTauriEvent('clipboard_changed', { revision: 2 })
    await flushPromises()
    expect(listCalls).toBe(2)

    await emitTauriEvent('clipboard_changed', { revision: 3 })
    await emitTauriEvent('clipboard_changed', { revision: 3 })
    inFlight.resolve(page({ revision: 2 }))
    await store.refresh(false)

    expect(listCalls).toBe(3)
    expect(store.loadedRevision).toBe(3)
    expect(store.items[0].id).toBe('latest')
  })

  it('discards an old query generation and applies the latest committed query', async () => {
    const firstSearch = deferred<ClipboardListPage>()
    const queries: unknown[] = []
    let listCalls = 0
    setTauriInvokeHandler((command, args) => {
      if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
      listCalls += 1
      queries.push(args?.query)
      if (listCalls === 1) return page()
      if (listCalls === 2) return firstSearch.promise
      return page({
        items: [createTextListItem({ id: 'beta', preview: {
          kind: 'text',
          mode: 'search_snippet',
          text: 'beta',
          highlight_ranges: [{ start: 0, end: 4 }],
        } })],
      })
    })

    const store = useClipboardViewStore()
    await store.start()
    store.setSearchInput('alpha')
    const alpha = store.applyCurrentFilter()
    await flushPromises()
    store.setSearchInput('beta')
    const beta = store.applyCurrentFilter()
    firstSearch.resolve(page({ items: [createTextListItem({ id: 'alpha' })] }))
    await Promise.all([alpha, beta])

    expect(queries).toEqual([
      { limit: 2 },
      { text: 'alpha', limit: 2 },
      { text: 'beta', limit: 2 },
    ])
    expect(store.items.map(({ id }) => id)).toEqual(['beta'])
  })

  it('commits text, date, tag, and type filters and clears them as one query', async () => {
    const queries: unknown[] = []
    setTauriInvokeHandler((command, args) => {
      if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
      queries.push(args?.query)
      return page({ items: [] })
    })

    const store = useClipboardViewStore()
    await store.start()
    store.setSearchInput('  needle  ')
    store.setSearchCommandFilter('type', 'image')
    store.setSearchCommandFilter('tag', 'url')
    await store.applyCurrentFilter('2026-07-12')

    expect(queries[1]).toEqual({
      text: 'needle',
      entryType: 'image',
      tag: 'url',
      date: '2026-07-12',
      limit: 2,
    })

    await store.clearSearch()
    expect(queries[2]).toEqual({ limit: 2 })
    expect(store.activeQuery).toEqual({})
    expect(store.selectedDate).toBeNull()
    expect(store.searchInput).toBe('')
  })

  it('appends cursor pages only when their revision matches the first page', async () => {
    const cursor = { createdAt: 100, id: 'first' }
    const queries: unknown[] = []
    setTauriInvokeHandler((command, args) => {
      if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
      queries.push(args?.query)
      if (queries.length === 1) {
        return page({
          revision: 8,
          items: [createTextListItem({ id: 'first' })],
          next_cursor: cursor,
          pinned_count: 2,
        })
      }
      return page({
        revision: 8,
        items: [
          createTextListItem({ id: 'first' }),
          createTextListItem({ id: 'second' }),
        ],
        next_cursor: null,
        pinned_count: 2,
      })
    })

    const store = useClipboardViewStore()
    await store.start()
    await store.loadMore()

    expect(queries[1]).toEqual({ cursor, limit: 2 })
    expect(store.items.map(({ id }) => id)).toEqual(['first', 'second'])
    expect(store.hasMore).toBe(false)
  })

  it('never appends an old cursor page after an invalidation refresh', async () => {
    const cursor = { createdAt: 100, id: 'first' }
    const oldCursorPage = deferred<ClipboardListPage>()
    let listCalls = 0
    setTauriInvokeHandler((command) => {
      if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
      listCalls += 1
      if (listCalls === 1) {
        return page({
          revision: 1,
          items: [createTextListItem({ id: 'first' })],
          next_cursor: cursor,
        })
      }
      if (listCalls === 2) return oldCursorPage.promise
      return page({
        revision: 2,
        items: [createTextListItem({ id: 'latest' })],
      })
    })

    const store = useClipboardViewStore()
    await store.start()
    const loadingMore = store.loadMore()
    await flushPromises()
    expect(listCalls).toBe(2)

    await emitTauriEvent('clipboard_changed', { revision: 2 })
    await flushPromises()
    expect(listCalls).toBe(2)

    oldCursorPage.resolve(page({
      revision: 1,
      items: [createTextListItem({ id: 'stale-page' })],
    }))
    await loadingMore
    await flushPromises()
    await flushPromises()

    expect(listCalls).toBe(3)
    expect(store.loadedRevision).toBe(2)
    expect(store.items.map(({ id }) => id)).toEqual(['latest'])
  })

  it.each([
    ['succeeds', false],
    ['fails', true],
  ] as const)(
    'refreshes a new query and releases pagination when the old page %s',
    async (_outcome, shouldFail) => {
      const oldCursor = { createdAt: 100, id: 'old-first' }
      const newCursor = { createdAt: 90, id: 'new-first' }
      const oldCursorPage = deferred<ClipboardListPage>()
      const queries: unknown[] = []
      let listCalls = 0
      setTauriInvokeHandler((command, args) => {
        if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
        listCalls += 1
        queries.push(args?.query)
        if (listCalls === 1) {
          return page({
            revision: 1,
            items: [createTextListItem({ id: 'old-first' })],
            next_cursor: oldCursor,
          })
        }
        if (listCalls === 2) return oldCursorPage.promise
        if (listCalls === 3) {
          return page({
            revision: 2,
            items: [createTextListItem({ id: 'new-first' })],
            next_cursor: newCursor,
          })
        }
        return page({
          revision: 2,
          items: [createTextListItem({ id: 'new-second' })],
          next_cursor: null,
        })
      })

      const store = useClipboardViewStore()
      await store.start()
      const oldPaginationOutcome = store.loadMore().then(
        () => null,
        (cause: unknown) => cause,
      )
      await flushPromises()

      store.setSearchInput('new query')
      const applyingQuery = store.applyCurrentFilter()
      await flushPromises()
      expect(store.loadingMore).toBe(true)
      expect(listCalls).toBe(2)

      if (shouldFail) {
        oldCursorPage.reject(new Error('old page failed'))
      } else {
        oldCursorPage.resolve(page({
          revision: 1,
          items: [createTextListItem({ id: 'old-second' })],
        }))
      }

      const oldOutcome = await oldPaginationOutcome
      await applyingQuery

      if (shouldFail) {
        expect(oldOutcome).toEqual(new Error('old page failed'))
      } else {
        expect(oldOutcome).toBeNull()
      }
      expect(store.loadingMore).toBe(false)
      expect(queries[2]).toEqual({ text: 'new query', limit: 2 })
      expect(store.items.map(({ id }) => id)).toEqual(['new-first'])

      await store.loadMore()

      expect(queries[3]).toEqual({ text: 'new query', cursor: newCursor, limit: 2 })
      expect(store.items.map(({ id }) => id)).toEqual(['new-first', 'new-second'])
      expect(store.hasMore).toBe(false)
    },
  )

  it('does not let a response clear a mutation invalidation raised while it was pending', async () => {
    const eventRefresh = deferred<ClipboardListPage>()
    let listCalls = 0
    setTauriInvokeHandler((command) => {
      if (command === 'get_clipboard_list_items') {
        listCalls += 1
        if (listCalls === 1) return page({ revision: 4 })
        if (listCalls === 2) return eventRefresh.promise
        return page({
          revision: 6,
          items: [createTextListItem({ id: 'after-mutation' })],
        })
      }
      if (command === 'delete_entry') return undefined
      throw new Error(`unexpected ${command}`)
    })

    const store = useClipboardViewStore()
    await store.start()
    await emitTauriEvent('clipboard_changed', { revision: 5 })
    await flushPromises()
    expect(listCalls).toBe(2)

    await store.remove('entry-1')
    eventRefresh.resolve(page({
      revision: 5,
      items: [createTextListItem({ id: 'before-mutation' })],
    }))
    await store.refresh(false)

    expect(listCalls).toBe(3)
    expect(store.loadedRevision).toBe(6)
    expect(store.items.map(({ id }) => id)).toEqual(['after-mutation'])
  })

  it('refreshes even when copying a missing image returns an error', async () => {
    let listCalls = 0
    setTauriInvokeHandler((command) => {
      if (command === 'get_clipboard_list_items') {
        listCalls += 1
        return listCalls === 1
          ? page({ items: [createTextListItem({ id: 'broken' })] })
          : page({ revision: 2, items: [] })
      }
      if (command === 'copy_entry') throw new Error('original missing')
      throw new Error(`unexpected ${command}`)
    })

    const store = useClipboardViewStore()
    await store.start()
    await expect(store.copy('broken')).rejects.toThrow('original missing')
    await flushPromises()

    expect(listCalls).toBe(2)
    expect(store.items).toEqual([])
    expect(store.loadedRevision).toBe(2)
  })

  it('handles repaired, removed, unchanged, and failed preview repair outcomes', async () => {
    let listCalls = 0
    let outcome: 'repaired' | 'removed' | 'unchanged' | 'error' = 'repaired'
    setTauriInvokeHandler((command) => {
      if (command === 'get_clipboard_list_items') {
        listCalls += 1
        return page({ revision: listCalls })
      }
      if (command === 'report_image_load_failed') {
        if (outcome === 'error') throw new Error('preview write failed')
        return outcome
      }
      throw new Error(`unexpected ${command}`)
    })

    const store = useClipboardViewStore()
    await store.start()

    expect(await store.repairImagePreview('image-1')).toBe('repaired')
    await flushPromises()
    expect(listCalls).toBe(2)

    outcome = 'removed'
    expect(await store.repairImagePreview('image-1')).toBe('removed')
    await flushPromises()
    expect(listCalls).toBe(3)

    outcome = 'unchanged'
    expect(await store.repairImagePreview('image-1')).toBe('unchanged')
    await flushPromises()
    expect(listCalls).toBe(3)

    outcome = 'error'
    await expect(store.repairImagePreview('image-1')).rejects.toThrow('preview write failed')
    await flushPromises()
    expect(listCalls).toBe(3)
  })

  it('releases its listener and loaded items on suspend, then reloads on resume', async () => {
    let listCalls = 0
    setTauriInvokeHandler((command) => {
      if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
      listCalls += 1
      return page({ revision: listCalls })
    })

    const store = useClipboardViewStore()
    await store.start()
    store.stop()

    expect(store.active).toBe(false)
    expect(store.items).toEqual([])

    await emitTauriEvent('clipboard_changed', { revision: 99 })
    expect(listCalls).toBe(1)

    await store.start()
    expect(listCalls).toBe(2)
    expect(store.active).toBe(true)
  })

  it('returns to inactive when listener setup fails and retries on the next start', async () => {
    tauriListenMock.mockRejectedValueOnce(new Error('listener unavailable'))
    let listCalls = 0
    setTauriInvokeHandler((command) => {
      if (command !== 'get_clipboard_list_items') throw new Error(`unexpected ${command}`)
      listCalls += 1
      return page({ revision: listCalls })
    })

    const store = useClipboardViewStore()
    await expect(store.start()).rejects.toThrow('listener unavailable')

    expect(store.active).toBe(false)
    expect(listCalls).toBe(0)

    await store.start()

    expect(tauriListenMock).toHaveBeenCalledTimes(2)
    expect(store.active).toBe(true)
    expect(listCalls).toBe(1)
  })
})
