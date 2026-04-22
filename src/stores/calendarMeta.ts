import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  fetchActiveDates as fetchActiveDatesApi,
  fetchEarliestMonth as fetchEarliestMonthApi,
} from '../composables/clipboardApi'

export const useCalendarMetaStore = defineStore('calendarMeta', () => {
  const earliestMonth = ref<string | null>(null)
  const calendarRevision = ref(0)

  async function refreshEarliestMonth() {
    earliestMonth.value = await fetchEarliestMonthApi()
  }

  async function refreshCalendarMeta() {
    await refreshEarliestMonth()
    calendarRevision.value += 1
  }

  function notifyCalendarDatesChanged() {
    calendarRevision.value += 1
  }

  async function fetchActiveDates(yearMonth: string) {
    return fetchActiveDatesApi(yearMonth)
  }

  return {
    earliestMonth,
    calendarRevision,
    refreshEarliestMonth,
    refreshCalendarMeta,
    notifyCalendarDatesChanged,
    fetchActiveDates,
  }
})
