<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { useI18n } from '../i18n'

const props = defineProps<{
  modelValue: string | null   // 已选日期 YYYY-MM-DD 或 null
  activeDates: string[]       // 有数据的日期，如 ["2026-04-01","2026-04-03"]
  disabledDate?: (dateStr: string) => boolean  // 返回 true 则禁用该日期
  max?: string                // 导航上限 YYYY-MM（默认不限制）
  min?: string                // 导航下限 YYYY-MM（默认不限制）
}>() 

const emit = defineEmits<{
  'update:modelValue': [date: string | null]
  'month-change': [yearMonth: string]  // "YYYY-MM" 格式
}>()

const { t, intlLocale, isZhLocale } = useI18n()

// ── State ────────────────────────────────────────────────────────────────────

const _today = new Date()
// 使用本地日期，toISOString() 返回 UTC 在 UTC+8 可能有偏差
const todayStr = [
  _today.getFullYear(),
  String(_today.getMonth() + 1).padStart(2, '0'),
  String(_today.getDate()).padStart(2, '0'),
].join('-')

function resolveVisibleMonth(date: string | null | undefined) {
  if (!date) {
    return { year: _today.getFullYear(), month: _today.getMonth() }
  }
  const [year, month] = date.split('-').map(Number)
  if (!year || !month) {
    return { year: _today.getFullYear(), month: _today.getMonth() }
  }
  return { year, month: month - 1 }
}

const initialMonth = resolveVisibleMonth(props.modelValue)
const currentYear = ref(initialMonth.year)
const currentMonth = ref(initialMonth.month) // 月份（从 0 开始的索引）

const yearMonth = computed(() => {
  const m = String(currentMonth.value + 1).padStart(2, '0')
  return `${currentYear.value}-${m}`
})

// 使用 Intl 生成本地化月份标题
const monthLabel = computed(() => {
  const d = new Date(currentYear.value, currentMonth.value, 1)
  return d.toLocaleDateString(intlLocale.value, { year: 'numeric', month: 'long' })
})

// 使用 Intl 生成本地化的星期缩写名
const WEEKDAYS_ZH = ['日', '一', '二', '三', '四', '五', '六']
const WEEKDAYS_EN = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa']
const weekdays = computed(() =>
  isZhLocale.value ? WEEKDAYS_ZH : WEEKDAYS_EN
)

const cells = computed(() => {
  const firstDay = new Date(currentYear.value, currentMonth.value, 1).getDay()
  const daysInMonth = new Date(currentYear.value, currentMonth.value + 1, 0).getDate()
  const result: Array<number | null> = []
  for (let i = 0; i < firstDay; i++) result.push(null)
  for (let d = 1; d <= daysInMonth; d++) result.push(d)
  return result
})

const activeSet = computed(() => new Set(props.activeDates))

function dateStr(day: number) {
  const m = String(currentMonth.value + 1).padStart(2, '0')
  const d = String(day).padStart(2, '0')
  return `${currentYear.value}-${m}-${d}`
}

function isSelected(day: number) {
  return props.modelValue === dateStr(day)
}

function hasData(day: number) {
  return activeSet.value.has(dateStr(day))
}

function isToday(day: number) {
  return dateStr(day) === todayStr
}

function isDisabled(day: number) {
  return props.disabledDate?.(dateStr(day)) ?? false
}

function isNextMonthDisabled() {
  if (!props.max) return false
  return yearMonth.value >= props.max
}

function isPrevMonthDisabled() {
  if (!props.min) return false
  return yearMonth.value <= props.min
}

function selectDay(day: number) {
  if (isDisabled(day)) return
  const ds = dateStr(day)
  emit('update:modelValue', ds === props.modelValue ? null : ds)
}

function prevMonth() {
  if (isPrevMonthDisabled()) return
  if (currentMonth.value === 0) {
    currentMonth.value = 11
    currentYear.value--
  } else {
    currentMonth.value--
  }
}

function nextMonth() {
  if (isNextMonthDisabled()) return
  if (currentMonth.value === 11) {
    currentMonth.value = 0
    currentYear.value++
  } else {
    currentMonth.value++
  }
}

watch(yearMonth, (ym) => emit('month-change', ym), { immediate: false })
watch(() => props.modelValue, (value) => {
  const next = resolveVisibleMonth(value)
  currentYear.value = next.year
  currentMonth.value = next.month
})
onMounted(() => emit('month-change', yearMonth.value))
</script>

<template>
  <div class="datepicker">
    <div class="dp-header">
      <button
        class="dp-nav-btn"
        :class="{ 'dp-nav-btn--disabled': isPrevMonthDisabled() }"
        @click="prevMonth"
      >‹</button>
      <span class="dp-month-label">{{ monthLabel }}</span>
      <button
        class="dp-nav-btn"
        :class="{ 'dp-nav-btn--disabled': isNextMonthDisabled() }"
        @click="nextMonth"
      >›</button>
    </div>

    <div class="dp-weekdays">
      <span v-for="wd in weekdays" :key="wd" class="dp-wd">{{ wd }}</span>
    </div>

    <div class="dp-grid">
      <div v-for="(cell, i) in cells" :key="i" class="dp-cell">
        <template v-if="cell !== null">
          <button
            @click="selectDay(cell)"
            :class="['dp-day', {
              'dp-day--selected': isSelected(cell),
              'dp-day--today': !isSelected(cell) && isToday(cell),
              'dp-day--disabled': isDisabled(cell),
            }]"
            :disabled="isDisabled(cell)"
          >
            {{ cell }}
          </button>
          <span
            class="dp-dot"
            :class="{ 'dp-dot--visible': hasData(cell), 'dp-dot--on-selected': isSelected(cell) && hasData(cell) }"
          />
        </template>
      </div>
    </div>

    <div v-if="modelValue" class="dp-clear">
      <button class="dp-clear-btn" @click="emit('update:modelValue', null)">
        {{ t('clearDateFilter') }} ✕
      </button>
    </div>
  </div>
</template>

<style scoped>
.datepicker {
  width: 256px;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-md);
  padding: var(--space-3);
  font-size: var(--font-size-sm);
  user-select: none;
}

.dp-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--space-2);
}

.dp-nav-btn {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 16px;
  transition: background 0.15s;
}

.dp-nav-btn:hover {
  background: var(--color-bg-hover);
}

.dp-month-label {
  font-size: var(--font-size-sm);
  font-weight: var(--font-weight-medium);
  color: var(--color-text-primary);
}

.dp-weekdays {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  margin-bottom: var(--space-1);
}

.dp-wd {
  text-align: center;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  padding: 2px 0;
}

.dp-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  row-gap: 2px;
}

.dp-cell {
  display: flex;
  flex-direction: column;
  align-items: center;
}

.dp-day {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 50%;
  font-size: var(--font-size-xs);
  color: var(--color-text-primary);
  background: transparent;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}

.dp-day:hover {
  background: var(--color-bg-hover);
}

.dp-day--selected {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
}

.dp-day--selected:hover {
  background: var(--color-accent-hover);
}

.dp-day--today {
  box-shadow: inset 0 0 0 1px var(--color-accent);
  color: var(--color-accent);
}

.dp-day--today:hover {
  background: var(--color-accent-subtle);
}

.dp-day--disabled {
  color: var(--color-text-tertiary);
  cursor: default;
  pointer-events: none;
}

.dp-nav-btn--disabled {
  color: var(--color-text-tertiary);
  cursor: default;
  pointer-events: none;
  opacity: 0.4;
}

.dp-dot {
  width: 4px;
  height: 4px;
  border-radius: 50%;
  margin-top: 1px;
  background: transparent;
}

.dp-dot--visible {
  background: var(--color-accent);
}

.dp-dot--on-selected {
  background: var(--color-text-on-accent);
}

.dp-clear {
  margin-top: var(--space-2);
  text-align: center;
}

.dp-clear-btn {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  border: none;
  background: transparent;
  cursor: pointer;
  transition: color 0.15s;
}

.dp-clear-btn:hover {
  color: var(--color-danger);
}
</style>
