import { computed } from 'vue'
import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardQueryStore } from '../stores/clipboardQuery'
import { useClipboardStreamBootstrap } from './useClipboardStreamBootstrap'

export function useClipboardSearchControls() {
  const calendarMetaStore = useCalendarMetaStore()
  const queryStore = useClipboardQueryStore()
  const streamBootstrap = useClipboardStreamBootstrap()

  const searchInput = computed({
    get: () => queryStore.searchInput,
    set: queryStore.setSearchInput,
  })
  const selectedDate = computed(() => queryStore.selectedDate)
  const searchCommandFilters = computed(() => queryStore.searchCommandFilters)
  const earliestMonth = computed(() => calendarMetaStore.earliestMonth)
  const calendarRevision = computed(() => calendarMetaStore.calendarRevision)

  async function applyCurrentFilter(date: string | null = queryStore.selectedDate) {
    await queryStore.applySearch(date)
    if (queryStore.isDefaultView) {
      await streamBootstrap.loadInitialStream()
    }
  }

  async function clearSearch() {
    await queryStore.clearSearch()
    await streamBootstrap.loadInitialStream()
  }

  async function refreshCalendarMeta() {
    await calendarMetaStore.refreshCalendarMeta()
  }

  async function fetchActiveDates(yearMonth: string) {
    return calendarMetaStore.fetchActiveDates(yearMonth)
  }

  return {
    searchInput,
    selectedDate,
    searchCommandFilters,
    earliestMonth,
    calendarRevision,
    setSearchInput: queryStore.setSearchInput,
    setSearchCommandFilter: queryStore.setSearchCommandFilter,
    clearSearchCommandFilter: queryStore.clearSearchCommandFilter,
    applyCurrentFilter,
    clearSearch,
    refreshCalendarMeta,
    fetchActiveDates,
  }
}
