import { watch } from 'vue'
import type { WatchStopHandle } from 'vue'
import { globalNow } from './useNow'
import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamStore } from '../stores/clipboardStream'

let stopTtlWatch: WatchStopHandle | null = null

export function useClipboardTtlVisibility() {
  const calendarMetaStore = useCalendarMetaStore()
  const queryStore = useClipboardQueryStore()
  const streamStore = useClipboardStreamStore()

  function handleTick(now: number) {
    const removedCount =
      streamStore.removeExpired(now).length +
      queryStore.removeExpired(now).length

    if (removedCount === 0) return

    void calendarMetaStore.refreshCalendarMeta().catch((error) => {
      console.error('[clipboard] failed to refresh calendar metadata after TTL expiry:', error)
    })
  }

  function start() {
    if (stopTtlWatch) return
    stopTtlWatch = watch(globalNow, handleTick, { immediate: true })
  }

  return { start }
}
