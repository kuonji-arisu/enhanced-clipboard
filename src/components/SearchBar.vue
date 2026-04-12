<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import DatePicker from './DatePicker.vue'
import Icon from './Icon.vue'
import SearchCommandMenu from './SearchCommandMenu.vue'
import SearchFilterChip from './SearchFilterChip.vue'
import { useSearchCommandPalette } from '../hooks/useSearchCommandPalette'
import { useCompositionGuard } from '../hooks/useCompositionGuard'
import { useAsyncAction } from '../hooks/useAsyncAction'
import { useClipboardStore } from '../stores/clipboard'
import { useI18n } from '../i18n'
import { debounce } from '../utils'

const { t } = useI18n()
const store = useClipboardStore()
const { run } = useAsyncAction()

const showCalendar = ref(false)
const activeDates = ref<string[]>([])
const visibleYearMonth = ref<string | null>(null)
const inputRef = ref<HTMLInputElement | null>(null)
const {
  onCompositionStart,
  onCompositionEnd: finishComposition,
  shouldSkipInputApply,
  isCompositionKeydown,
} = useCompositionGuard()

const applyFilter = debounce(() => {
  void run(() => store.applySearch(), 'loadEntriesFailed')
}, 300)

function onInput(event: Event) {
  const input = event.target as HTMLInputElement
  store.setSearchInput(input.value)
  if (shouldSkipInputApply()) {
    return
  }
  applyFilter()
}

function onCompositionEnd(event: CompositionEvent) {
  finishComposition()
  const input = event.target as HTMLInputElement | null
  if (!input) {
    applyFilter()
    return
  }
  store.setSearchInput(input.value)
  applyFilter()
}

const {
  activeFilterChips,
  showCommandMenu,
  commandMenuTitle,
  commandDraft,
  commandOptions,
  activeCommandValue,
  onFocus,
  onBlur,
  selectCommand,
  clearFilter,
  onInputKeydown,
} = useSearchCommandPalette({
  inputRef,
  searchInput: computed(() => store.searchInput),
  searchCommandFilters: computed(() => store.searchCommandFilters),
  isCompositionKeydown,
  applyFilter,
  setSearchInput: store.setSearchInput,
  setSearchCommandFilter: store.setSearchCommandFilter,
  clearSearchCommandFilter: store.clearSearchCommandFilter,
})

function onDateChange(date: string | null) {
  showCalendar.value = false
  void run(() => store.applySearch(date), 'loadEntriesFailed')
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
    <div class="searchbar-main">
      <div class="searchbar-input-shell">
        <span v-if="activeFilterChips.length === 0" class="searchbar-icon">
          <Icon name="search" :size="13" />
        </span>
        <div v-if="activeFilterChips.length > 0" class="searchbar-chip-inline">
          <SearchFilterChip
            v-for="chip in activeFilterChips"
            :key="chip.key"
            :label="chip.label"
            @remove="clearFilter(chip.key)"
          />
        </div>
        <input
          ref="inputRef"
          @input="onInput"
          @compositionstart="onCompositionStart"
          @compositionend="onCompositionEnd"
          @focus="onFocus"
          @blur="onBlur"
          @keydown="onInputKeydown"
          type="text"
          :value="store.searchInput"
          :placeholder="t('searchCommandPlaceholder')"
          class="searchbar-input"
        />

        <SearchCommandMenu
          :visible="showCommandMenu"
          :title="commandMenuTitle"
          :query="commandDraft"
          :options="commandOptions"
          :active-value="activeCommandValue?.value ?? null"
          @select="selectCommand"
        />
      </div>

      <button
        @click.stop="toggleCalendar"
        :class="['cal-btn', { 'cal-btn--active': store.selectedDate }]"
      >
        <Icon name="calendar" :size="14" />
      </button>
    </div>

    <div v-if="showCalendar" v-click-outside="closeCalendar" class="calendar-popover">
      <DatePicker
        :model-value="store.selectedDate"
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
  flex-direction: column;
  gap: var(--space-1);
}

.searchbar-main {
  display: flex;
  align-items: center;
  gap: var(--space-1);
}

.searchbar-input-shell {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 32px;
  padding: 0 var(--space-3);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  transition: border-color 0.15s, box-shadow 0.15s;
}

.searchbar-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  font-size: var(--font-size-sm);
}

.searchbar-chip-inline {
  display: inline-flex;
  align-items: center;
  min-width: 0;
  flex-shrink: 0;
  max-width: 50%;
}

.searchbar-input {
  flex: 1;
  min-width: 0;
  width: 100%;
  padding: 0;
  background: transparent;
  border: none;
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  outline: none;
}

.searchbar-input::placeholder {
  color: var(--color-text-tertiary);
}

.searchbar-input-shell:focus-within {
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

