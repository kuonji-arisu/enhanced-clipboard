<script setup lang="ts">
import { computed } from 'vue'
import type { TextRange } from '../types'

interface Segment {
  text: string
  highlighted: boolean
}

const props = defineProps<{
  text: string
  ranges?: TextRange[]
}>()

function buildSegmentsFromRanges(text: string, ranges: TextRange[]): Segment[] {
  if (!ranges.length) return [{ text, highlighted: false }]
  const chars = Array.from(text)
  const segments: Segment[] = []
  let cursor = 0

  for (const range of ranges) {
    const start = Math.max(cursor, Math.min(range.start, chars.length))
    const end = Math.max(start, Math.min(range.end, chars.length))
    if (start > cursor) {
      segments.push({ text: chars.slice(cursor, start).join(''), highlighted: false })
    }
    if (end > start) {
      segments.push({ text: chars.slice(start, end).join(''), highlighted: true })
    }
    cursor = end
  }

  if (cursor < chars.length) {
    segments.push({ text: chars.slice(cursor).join(''), highlighted: false })
  }
  return segments.length > 0 ? segments : [{ text, highlighted: false }]
}

const segments = computed(() => buildSegmentsFromRanges(props.text, props.ranges ?? []))
</script>

<template>
  <span>
    <span
      v-for="(segment, index) in segments"
      :key="`${index}-${segment.highlighted ? 'hit' : 'text'}`"
      :class="{ 'highlighted-text__mark': segment.highlighted }"
    >
      {{ segment.text }}
    </span>
  </span>
</template>

<style scoped>
.highlighted-text__mark {
  padding: 0 1px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--color-accent) 14%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-accent) 16%, transparent);
  color: inherit;
  font-weight: 600;
}
</style>
