<script setup lang="ts">
import { computed } from 'vue'

interface Segment {
  text: string
  highlighted: boolean
}

interface CharRange {
  start: number
  end: number
}

const props = defineProps<{
  text: string
  query: string
}>()

function normalizePreviewQuery(query: string): string {
  return query.replace(/\s+/gu, ' ').trim()
}

function findCaseSensitiveCharRange(text: string, query: string): CharRange | null {
  const matchIndex = text.indexOf(query)
  if (matchIndex === -1) {
    return null
  }

  const start = Array.from(text.slice(0, matchIndex)).length
  const end = start + Array.from(query).length
  return { start, end }
}

function findLowercaseFallbackCharRange(text: string, query: string): CharRange | null {
  const loweredTextParts: string[] = []
  const loweredToOriginalChar: number[] = []

  Array.from(text).forEach((char, originalCharIndex) => {
    for (const lower of Array.from(char.toLowerCase())) {
      loweredTextParts.push(lower)
      loweredToOriginalChar.push(originalCharIndex)
    }
  })

  const loweredQueryChars = Array.from(query).flatMap((char) => Array.from(char.toLowerCase()))
  if (loweredQueryChars.length === 0) {
    return null
  }

  const loweredText = loweredTextParts.join('')
  const loweredQuery = loweredQueryChars.join('')
  const loweredMatchIndex = loweredText.indexOf(loweredQuery)
  if (loweredMatchIndex === -1) {
    return null
  }

  const loweredCharStart = Array.from(loweredText.slice(0, loweredMatchIndex)).length
  const loweredCharEnd = loweredCharStart + loweredQueryChars.length
  const originalStart = loweredToOriginalChar[loweredCharStart]
  const originalEnd = loweredToOriginalChar[loweredCharEnd - 1]

  if (originalStart == null || originalEnd == null) {
    return null
  }

  return { start: originalStart, end: originalEnd + 1 }
}

function findFirstMatchCharRange(text: string, query: string): CharRange | null {
  return (
    findCaseSensitiveCharRange(text, query) ??
    findLowercaseFallbackCharRange(text, query)
  )
}

function buildSegments(text: string, query: string): Segment[] {
  const normalizedQuery = normalizePreviewQuery(query)
  if (!normalizedQuery) {
    return [{ text, highlighted: false }]
  }

  const textChars = Array.from(text)
  const segments: Segment[] = []
  let searchStart = 0

  while (searchStart < textChars.length) {
    const remainingText = textChars.slice(searchStart).join('')
    const match = findFirstMatchCharRange(remainingText, normalizedQuery)

    if (!match) {
      if (searchStart < textChars.length) {
        segments.push({
          text: textChars.slice(searchStart).join(''),
          highlighted: false,
        })
      }
      break
    }

    const matchStart = searchStart + match.start
    const matchEnd = searchStart + match.end

    if (matchStart > searchStart) {
      segments.push({
        text: textChars.slice(searchStart, matchStart).join(''),
        highlighted: false,
      })
    }

    segments.push({
      text: textChars.slice(matchStart, matchEnd).join(''),
      highlighted: true,
    })
    searchStart = matchEnd
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
