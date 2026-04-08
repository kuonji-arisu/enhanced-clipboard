<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import DatePicker from './DatePicker.vue'
import Icon from './Icon.vue'
import { useAsyncAction } from '../hooks/useAsyncAction'
import { useClipboardStore } from '../stores/clipboard'
import { useI18n } from '../i18n'
import { debounce } from '../utils'

const { t } = useI18n()
const store = useClipboardStore()
const { run } = useAsyncAction()

const query = ref(store.searchQuery)
const selectedDate = ref<string | null>(store.selectedDate)
const showCalendar = ref(false)
const activeDates = ref<string[]>([])
const visibleYearMonth = ref<string | null>(null)

const applyFilter = debounce(() => {
  void run(() => store.setFilter(query.value, selectedDate.value), 'loadEntriesFailed')
}, 300)

function onInput() {
  applyFilter()
}

function onDateChange(date: string | null) {
  selectedDate.value = date
  showCalendar.value = false
  void run(() => store.setFilter(query.value, date), 'loadEntriesFailed')
}

async function onMonthChange(yearMonth: string) {
  visibleYearMonth.value = yearMonth
  const dates = await run(() => store.fetchActiveDates(yearMonth), 'calendarLoadFailed')
  if (dates) {
    activeDates.value = dates
  }
}

async function toggleCalendar() {
  showCalendar.value = !showCalendar.value
  if (showCalendar.value) {
    await run(() => store.refreshCalendarMeta(), 'calendarLoadFailed')
  }
}

function closeCalendar() {
  showCalendar.value = false
}

const todayYearMonth = computed(() => {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`
})

// 仅允许选择有数据的日期
const activeSet = computed(() => new Set(activeDates.value))
function disabledDate(dateStr: string) {
  return !activeSet.value.has(dateStr)
}

watch(
  () => store.calendarRevision,
  (revision, previous) => {
    if (revision === previous) return
    if (!showCalendar.value || !visibleYearMonth.value) return
    void onMonthChange(visibleYearMonth.value)
  },
)
</script>

<template>
  <div class="searchbar">
    <div class="searchbar-input-wrap">
      <span class="searchbar-icon">
        <Icon name="search" :size="13" />
      </span>
      <input
        v-model="query"
        @input="onInput"
        type="text"
        :placeholder="t('search')"
        class="searchbar-input"
      />
    </div>

    <button
      @click.stop="toggleCalendar"
      :class="['cal-btn', { 'cal-btn--active': selectedDate }]"
      :title="t('filterByDate')"
    >
      <Icon name="calendar" :size="14" />
    </button>

    <div v-if="showCalendar" v-click-outside="closeCalendar" class="calendar-popover">
      <DatePicker
        :model-value="selectedDate"
        :active-dates="activeDates"
        :disabled-date="disabledDate"
        :max="todayYearMonth"
        :min="store.earliestMonth ?? undefined"
        @update:model-value="onDateChange"
        @month-change="onMonthChange"
      />
    </div>
  </div>
</template>

<style scoped>
.searchbar {
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--space-1);
}

.searchbar-input-wrap {
  position: relative;
  flex: 1;
}

.searchbar-icon {
  position: absolute;
  left: var(--space-3);
  top: 50%;
  transform: translateY(-50%);
  font-size: var(--font-size-sm);
  pointer-events: none;
  color: var(--color-text-tertiary);
}

.searchbar-input {
  width: 100%;
  padding: 6px var(--space-3) 6px 32px;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
}

.searchbar-input::placeholder {
  color: var(--color-text-tertiary);
}

.searchbar-input:focus {
  border-color: var(--color-accent);
  box-shadow: 0 0 0 3px var(--color-accent-subtle);
}

.cal-btn {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
  color: var(--color-text-tertiary);
  cursor: pointer;
  flex-shrink: 0;
  transition: border-color 0.15s, background 0.15s, color 0.15s;
  font-size: 14px;
}

.cal-btn:hover {
  border-color: var(--color-border-strong);
  color: var(--color-text-primary);
}

.cal-btn--active {
  border-color: var(--color-accent);
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}

.calendar-popover {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  z-index: 50;
}
</style>

