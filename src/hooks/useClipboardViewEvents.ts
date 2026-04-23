import type { UnlistenFn } from '@tauri-apps/api/event'
import { listenClipboardEvents } from '../composables/clipboardApi'
import { CLIPBOARD_QUERY_STALE_REASON } from '../types'
import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'
import { handleSettingsDrivenVisibilityStale } from '../utils/clipboardViewCoordinator'

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
      onQueryResultsStale: (reason) => {
        queryStore.markStale(reason)
        if (reason !== CLIPBOARD_QUERY_STALE_REASON.SETTINGS_OR_STARTUP) {
          return
        }

        void (async () => {
          try {
            await handleSettingsDrivenVisibilityStale()
          } catch (error) {
            console.error('[clipboard] failed to reconcile settings-driven list changes:', error)
          }
        })()
      },
    })
  }

  return { start }
}
