<script setup lang="ts">
defineProps<{
  visible: boolean
  title: string
  query: string
  options: Array<{
    value: string
    token: string
    label: string
  }>
  activeValue: string | null
}>()

const emit = defineEmits<{
  select: [value: string]
}>()
</script>

<template>
  <div v-if="visible && options.length > 0" class="search-command-menu">
    <div class="search-command-menu__header">
      <span class="search-command-menu__title">{{ title }}</span>
      <span v-if="query" class="search-command-menu__query">{{ query }}</span>
    </div>
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
      <span class="search-command-menu__token">{{ option.token }}</span>
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

.search-command-menu__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 2px 6px 6px;
  border-bottom: 1px solid color-mix(in srgb, var(--color-border) 70%, transparent);
}

.search-command-menu__title {
  font-size: 11px;
  font-weight: var(--font-weight-medium);
  color: var(--color-text-secondary);
}

.search-command-menu__query {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 11px;
  color: var(--color-text-tertiary);
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
  color: var(--color-text-secondary);
}

.search-command-menu__label {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}

.search-command-menu__item:hover .search-command-menu__label,
.search-command-menu__item--active .search-command-menu__label {
  color: inherit;
}
</style>
