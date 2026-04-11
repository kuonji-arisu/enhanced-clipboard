<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from '../i18n'
import type { EntrySearchTypeValue } from '../utils/entrySearchCommands'

const props = defineProps<{
  visible: boolean
  options: EntrySearchTypeValue[]
  activeValue: EntrySearchTypeValue | null
}>()

const emit = defineEmits<{
  select: [value: EntrySearchTypeValue]
}>()

const { t } = useI18n()

const options = computed(() =>
  props.options.map((value) => ({
    value,
    label: value === 'text' ? t('searchTypeText') : t('searchTypeImage'),
  })),
)
</script>

<template>
  <div v-if="visible && options.length > 0" class="search-command-menu">
    <button
      v-for="option in options"
      :key="option.value"
      :class="[
        'search-command-menu__item',
        { 'search-command-menu__item--active': option.value === activeValue },
      ]"
      type="button"
      @mousedown.prevent
      @click="emit('select', option.value)"
    >
      <span class="search-command-menu__token">type:{{ option.value }}</span>
      <span class="search-command-menu__label">{{ option.label }}</span>
    </button>
  </div>
</template>

<style scoped>
.search-command-menu {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  z-index: 60;
  min-width: 220px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 6px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--color-bg-overlay) 96%, var(--color-bg-elevated));
  box-shadow: var(--shadow-md);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
}

.search-command-menu__item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  border: none;
  border-radius: var(--radius-sm);
  padding: 8px 10px;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  text-align: left;
  transition: background 0.15s, color 0.15s;
}

.search-command-menu__item:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.search-command-menu__item--active {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.search-command-menu__token {
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 11px;
}

.search-command-menu__label {
  font-size: var(--font-size-xs);
}
</style>
