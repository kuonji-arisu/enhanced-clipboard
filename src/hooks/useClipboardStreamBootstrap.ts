import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardStreamStore } from '../stores/clipboardStream'

export function useClipboardStreamBootstrap() {
  const calendarMetaStore = useCalendarMetaStore()
  const streamStore = useClipboardStreamStore()

  async function loadInitialStream() {
    await streamStore.loadInitial()
    await calendarMetaStore.refreshEarliestMonth()
  }

  return { loadInitialStream }
}
