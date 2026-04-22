import type { UnlistenFn } from '@tauri-apps/api/event'
import { listenClipboardEvents } from '../composables/clipboardApi'
import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'

let unlisten: UnlistenFn | null = null

// Coordinates view-facing clipboard events across the stream, snapshot, and
// calendar stores. Snapshot views reproject known items through the backend;
// typed stale reasons remain owned by the backend query-stale event.
export function useClipboardViewEvents() {
  const calendarMetaStore = useCalendarMetaStore()
  const queryStore = useClipboardQueryStore()
  const streamStore = useClipboardStreamStore()

  async function start() {
    if (unlisten) return

    unlisten = await listenClipboardEvents({
      onStreamItemAdded: (item) => {
        streamStore.applyStreamItemAdded(item)
        calendarMetaStore.notifyCalendarDatesChanged()
      },
      onStreamItemUpdated: (item) => {
        streamStore.applyStreamItemUpdated(item)
        void queryStore.refreshKnownItem(item.id).catch((error) => {
          console.error('[clipboard] failed to refresh snapshot item:', error)
        })
      },
      onEntriesRemoved: (ids) => {
        streamStore.removeIds(ids)
        queryStore.removeKnownIds(ids)
        void calendarMetaStore.refreshCalendarMeta().catch((error) => {
          console.error('[clipboard] failed to refresh calendar metadata:', error)
        })
      },
      onQueryResultsStale: queryStore.markStale,
    })
  }

  return { start }
}
