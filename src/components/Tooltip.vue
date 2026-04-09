<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'

const props = withDefaults(defineProps<{
  content?: string
  placement?: 'top' | 'bottom'
  delay?: number
}>(), {
  content: '',
  placement: 'top',
  delay: 1000,
})

/** 与视口边缘的最小安全距离（px） */
const VIEWPORT_MARGIN = 8
/** tooltip 与锚点的间距（px） */
const GAP = 6

const anchorRef = ref<HTMLElement | null>(null)
const tooltipRef = ref<HTMLElement | null>(null)
const show = ref(false)
const ready = ref(false)
const pos = ref({ x: 0, y: 0 })
let frameId = 0
let openTimer: number | null = null

async function showTooltip() {
  if (!props.content) return
  show.value = true
  ready.value = false
  await nextTick()
  calculate()
  ready.value = true
}

function close() {
  clearOpenTimer()
  show.value = false
}

function clearOpenTimer() {
  if (openTimer !== null) {
    window.clearTimeout(openTimer)
    openTimer = null
  }
}

function openWithDelay() {
  if (!props.content) return
  clearOpenTimer()
  if (show.value) return
  if (props.delay <= 0) {
    void showTooltip()
    return
  }
  openTimer = window.setTimeout(() => {
    openTimer = null
    void showTooltip()
  }, props.delay)
}

function openImmediately() {
  clearOpenTimer()
  void showTooltip()
}

function calculate() {
  const anchor = anchorRef.value
  const tip = tooltipRef.value
  if (!anchor || !tip) return

  const ar = anchor.getBoundingClientRect()
  const tw = tip.offsetWidth
  const th = tip.offsetHeight
  const vw = window.innerWidth
  const vh = window.innerHeight

  // 水平：锚点居中对齐，clamp 到 [margin, vw - tw - margin]
  const x = Math.max(
    VIEWPORT_MARGIN,
    Math.min(ar.left + ar.width / 2 - tw / 2, vw - tw - VIEWPORT_MARGIN),
  )

  const topY = ar.top - th - GAP
  const bottomY = ar.bottom + GAP
  const preferTop = props.placement === 'top'

  let y = preferTop ? topY : bottomY
  if (preferTop && topY < VIEWPORT_MARGIN) {
    y = bottomY
  } else if (!preferTop && bottomY + th > vh - VIEWPORT_MARGIN) {
    y = topY
  }

  y = Math.max(VIEWPORT_MARGIN, Math.min(y, vh - th - VIEWPORT_MARGIN))

  pos.value = { x, y }
}

function queueCalculate() {
  if (frameId) return
  frameId = window.requestAnimationFrame(() => {
    frameId = 0
    calculate()
  })
}

function handleFocusOut(event: FocusEvent) {
  const nextTarget = event.relatedTarget
  if (
    nextTarget instanceof Node &&
    anchorRef.value &&
    anchorRef.value.contains(nextTarget)
  ) {
    return
  }
  close()
}

function removeListeners() {
  window.removeEventListener('resize', queueCalculate)
  window.removeEventListener('scroll', queueCalculate, true)
  if (frameId) {
    window.cancelAnimationFrame(frameId)
    frameId = 0
  }
}

watch(show, (visible) => {
  if (!visible) {
    removeListeners()
    return
  }

  window.addEventListener('resize', queueCalculate)
  window.addEventListener('scroll', queueCalculate, true)
})

onBeforeUnmount(() => {
  clearOpenTimer()
  removeListeners()
})
</script>

<template>
  <span
    ref="anchorRef"
    class="tooltip-trigger"
    @pointerenter="openWithDelay"
    @pointerleave="close"
    @focusin="openImmediately"
    @focusout="handleFocusOut"
  >
    <slot />
    <Teleport to="body">
      <span
        v-if="show && props.content"
        ref="tooltipRef"
        class="tooltip-float"
        :class="{ 'tooltip-float--ready': ready }"
        :style="{ left: `${pos.x}px`, top: `${pos.y}px` }"
      >{{ props.content }}</span>
    </Teleport>
  </span>
</template>

<style scoped>
.tooltip-trigger {
  display: inline-flex;
  vertical-align: top;
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
