<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  tag: string
}>()

const label = computed(() => formatEntryTagLabel(props.tag))

function formatEntryTagLabel(tag: string): string {
  switch (tag) {
    case 'json':
      return 'JSON'
    case 'url':
      return 'URL'
    case 'email':
      return 'Email'
    default:
      return tag
  }
}
</script>

<template>
  <span class="entry-tag-chip">{{ label }}</span>
</template>

<style scoped>
.entry-tag-chip {
  display: inline-flex;
  align-items: center;
  min-height: 18px;
  padding: 0 6px;
  border: 1px solid color-mix(in srgb, var(--color-accent) 4%, var(--color-border));
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-accent-subtle) 8%, var(--color-bg-hover));
  color: color-mix(in srgb, var(--color-text-secondary) 72%, var(--color-text-tertiary));
  font-size: 9px;
  font-weight: var(--font-weight-medium);
  line-height: 1;
  letter-spacing: 0.03em;
  text-transform: uppercase;
  white-space: nowrap;
  box-shadow: inset 0 1px 0 color-mix(in srgb, white 10%, transparent);
}

[data-theme="dark"] .entry-tag-chip {
  box-shadow: inset 0 1px 0 color-mix(in srgb, white 6%, transparent);
}
</style>
