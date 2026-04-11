<script setup lang="ts">
import { useI18n } from '../i18n'
import type { EntrySearchTypeValue } from '../utils/entrySearchCommands'

defineProps<{
  value: EntrySearchTypeValue
}>()

const emit = defineEmits<{
  remove: []
}>()

const { t } = useI18n()
</script>

<template>
  <button class="search-type-chip" type="button" @mousedown.prevent @click="emit('remove')">
    <span class="search-type-chip__value">
      {{ value === 'text' ? t('searchTypeText') : t('searchTypeImage') }}
    </span>
    <span class="search-type-chip__close">×</span>
  </button>
</template>

<style scoped>
.search-type-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 100%;
  min-height: 22px;
  border: 1px solid color-mix(in srgb, var(--color-accent) 10%, var(--color-border));
  border-radius: 8px;
  padding: 1px 8px;
  background: color-mix(in srgb, var(--color-accent-subtle) 22%, var(--color-bg-elevated));
  color: var(--color-text-primary);
  box-shadow: inset 0 1px 0 color-mix(in srgb, white 18%, transparent);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s, box-shadow 0.15s;
}

.search-type-chip:hover {
  border-color: color-mix(in srgb, var(--color-accent) 18%, var(--color-border));
  background: color-mix(in srgb, var(--color-accent-subtle) 34%, var(--color-bg-elevated));
  box-shadow: inset 0 1px 0 color-mix(in srgb, white 24%, transparent);
}

.search-type-chip__value {
  white-space: nowrap;
  font-size: 11px;
  line-height: 1;
  font-weight: var(--font-weight-medium);
}

.search-type-chip__close {
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 10px;
  height: 10px;
  font-size: 10px;
  line-height: 1;
  color: var(--color-text-tertiary);
  transform: translateY(-0.5px);
}
</style>
