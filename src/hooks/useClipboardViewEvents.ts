import type { UnlistenFn } from '@tauri-apps/api/event'
import { listenClipboardEvents } from '../composables/clipboardApi'
import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'
import { CLIPBOARD_QUERY_STALE_REASON } from '../types'

let unlisten: UnlistenFn | null = null

// Coordinates view-facing clipboard events across the stream, snapshot, and
// calendar stores. The individual stores stay focused on their own state.
export function useClipboardViewEvents() {
  const calendarMetaStore = useCalendarMetaStore()
  const queryStore = useClipboardQueryStore()
  const streamStore = useClipboardStreamStore()

  async function start() {
    if (unlisten) return

    unlisten = await listenClipboardEvents({
      onStreamItemAdded: (item) => {
        streamStore.applyStreamItemAdded(item)
        queryStore.markStale(CLIPBOARD_QUERY_STALE_REASON.ENTRY_CREATED)
        calendarMetaStore.notifyCalendarDatesChanged()
      },
      onStreamItemUpdated: (item) => {
        streamStore.applyStreamItemUpdated(item)
        queryStore.markStale(CLIPBOARD_QUERY_STALE_REASON.ENTRY_UPDATED)
      },
      onEntriesRemoved: (ids) => {
        streamStore.removeIds(ids)
        queryStore.removeKnownIds(ids)
        queryStore.markStale(CLIPBOARD_QUERY_STALE_REASON.ENTRIES_REMOVED)
        void calendarMetaStore.refreshCalendarMeta().catch((error) => {
          console.error('[clipboard] failed to refresh calendar metadata:', error)
        })
      },
      onQueryResultsStale: queryStore.markStale,
    })
  }

  return { start }
}
