<script setup lang="ts">
import { computed } from 'vue'

interface Segment {
  text: string
  highlighted: boolean
}

const props = defineProps<{
  text: string
  query: string
}>()

function buildSegments(text: string, query: string): Segment[] {
  const trimmedQuery = query.trim()
  if (!trimmedQuery) {
    return [{ text, highlighted: false }]
  }

  const loweredText = text.toLowerCase()
  const loweredQuery = trimmedQuery.toLowerCase()
  const segments: Segment[] = []
  let cursor = 0

  while (cursor < text.length) {
    const matchIndex = loweredText.indexOf(loweredQuery, cursor)
    if (matchIndex === -1) {
      if (cursor < text.length) {
        segments.push({
          text: text.slice(cursor),
          highlighted: false,
        })
      }
      break
    }

    if (matchIndex > cursor) {
      segments.push({
        text: text.slice(cursor, matchIndex),
        highlighted: false,
      })
    }

    segments.push({
      text: text.slice(matchIndex, matchIndex + trimmedQuery.length),
      highlighted: true,
    })
    cursor = matchIndex + trimmedQuery.length
  }

  return segments.length > 0 ? segments : [{ text, highlighted: false }]
}

const segments = computed(() => buildSegments(props.text, props.query))
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
