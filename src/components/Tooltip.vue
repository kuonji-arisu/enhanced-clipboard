<script setup lang="ts">
import { ref, nextTick } from 'vue'

defineProps<{ content: string }>()

/** 与视口边缘的最小安全距离（px） */
const VIEWPORT_MARGIN = 8
/** tooltip 与锚点的间距（px） */
const GAP = 6

const anchorRef = ref<HTMLElement | null>(null)
const tooltipRef = ref<HTMLElement | null>(null)
const show = ref(false)
const ready = ref(false)
const pos = ref({ x: 0, y: 0 })

async function open() {
  show.value = true
  ready.value = false
  await nextTick()
  calculate()
  ready.value = true
}

function close() {
  show.value = false
}

function calculate() {
  const anchor = anchorRef.value
  const tip = tooltipRef.value
  if (!anchor || !tip) return

  const ar = anchor.getBoundingClientRect()
  const tw = tip.offsetWidth
  const th = tip.offsetHeight
  const vw = window.innerWidth

  // 水平：锚点居中对齐，clamp 到 [margin, vw - tw - margin]
  const x = Math.max(
    VIEWPORT_MARGIN,
    Math.min(ar.left + ar.width / 2 - tw / 2, vw - tw - VIEWPORT_MARGIN),
  )

  // 垂直：优先上方，空间不足翻转到下方（并确保不超出视口底部）
  const y = ar.top - th - GAP >= VIEWPORT_MARGIN
    ? ar.top - th - GAP
    : Math.min(ar.bottom + GAP, window.innerHeight - th - VIEWPORT_MARGIN)

  pos.value = { x, y }
}
</script>

<template>
  <span
    ref="anchorRef"
    class="tooltip-trigger"
    @mouseenter="open"
    @mouseleave="close"
    @focusin="open"
    @focusout="close"
  >
    <slot />
    <Teleport to="body">
      <span
        v-if="show && content"
        ref="tooltipRef"
        class="tooltip-float"
        :class="{ 'tooltip-float--ready': ready }"
        :style="{ left: `${pos.x}px`, top: `${pos.y}px` }"
      >{{ content }}</span>
    </Teleport>
  </span>
</template>

<style scoped>
.tooltip-trigger {
  display: inline-flex;
  max-width: 100%;
}

.tooltip-float {
  position: fixed;
  z-index: 9999;
  white-space: nowrap;
  font-size: var(--font-size-xs);
  color: var(--color-text-primary);
  background: var(--color-bg-tooltip);
  border-radius: var(--radius-md);
  padding: 4px 10px;
  box-shadow: var(--shadow-md);
  pointer-events: none;
  opacity: 0;
  transition: opacity 0.12s ease;
}

.tooltip-float--ready {
  opacity: 1;
}
</style>
