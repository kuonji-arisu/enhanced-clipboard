import { computed, nextTick, onBeforeUnmount, reactive, ref, watch, type Ref } from 'vue'
import {
  COPY_FEEDBACK_MS,
  ENTRY_ENTER_ANIMATION_MS,
  ENTRY_EXIT_ANIMATION_MS,
  ENTRY_PIN_FEEDBACK_MS,
} from '../constants'
import { useAsyncAction } from './useAsyncAction'
import { useClipboardStore } from '../stores/clipboard'
import type { ClipboardEntry } from '../types'

export function useEntryAnimations(entry: Ref<ClipboardEntry>) {
  const store = useClipboardStore()
  const { run } = useAsyncAction()
  const copied = ref(false)
  const pinFeedback = ref<'on' | 'off' | null>(null)

  const imageProcessing = computed(
    () => entry.value.content_type === 'image' && !entry.value.thumbnail_path,
  )
  const deleting = computed(() => store.isDeleting(entry.value.id))
  const pinning = computed(() => store.isPinPending(entry.value.id))
  const actionDisabled = computed(() => deleting.value || pinning.value)
  const pinButtonDisabled = computed(() => deleting.value)
  const motionVars = computed(() => ({
    '--entry-exit-duration': `${ENTRY_EXIT_ANIMATION_MS}ms`,
    '--entry-pin-duration': `${ENTRY_PIN_FEEDBACK_MS}ms`,
  }))

  let copyTimer: number | null = null
  let pinFeedbackTimer: number | null = null

  async function handleCopy() {
    const copiedOk = await run(() => store.copy(entry.value.id).then(() => true), 'copyFailed')
    if (!copiedOk) return

    copied.value = true
    if (copyTimer) window.clearTimeout(copyTimer)
    copyTimer = window.setTimeout(() => {
      copied.value = false
      copyTimer = null
    }, COPY_FEEDBACK_MS)
  }

  async function handleDelete() {
    if (deleting.value) return
    await run(() => store.removeWithDelay(entry.value.id, ENTRY_EXIT_ANIMATION_MS), 'deleteFailed')
  }

  async function handlePin() {
    if (pinning.value) return

    const nextPinnedState = !entry.value.is_pinned
    pinFeedback.value = nextPinnedState ? 'on' : 'off'
    if (pinFeedbackTimer) window.clearTimeout(pinFeedbackTimer)
    pinFeedbackTimer = window.setTimeout(() => {
      pinFeedback.value = null
      pinFeedbackTimer = null
    }, ENTRY_PIN_FEEDBACK_MS)

    const updated = await run(
      () => store.togglePinWithDelay(entry.value.id, ENTRY_PIN_FEEDBACK_MS),
      'pinFailed',
    )

    if (updated === undefined && pinFeedbackTimer) {
      window.clearTimeout(pinFeedbackTimer)
      pinFeedback.value = null
      pinFeedbackTimer = null
    }
  }

  onBeforeUnmount(() => {
    if (copyTimer) window.clearTimeout(copyTimer)
    if (pinFeedbackTimer) window.clearTimeout(pinFeedbackTimer)
  })

  return {
    copied,
    pinFeedback,
    imageProcessing,
    deleting,
    pinning,
    actionDisabled,
    pinButtonDisabled,
    motionVars,
    handleCopy,
    handleDelete,
    handlePin,
  }
}

export function useVisibleEntryEnter(visibleEntryIds: Ref<string[]>) {
  const store = useClipboardStore()
  const enteringIds = reactive(new Set<string>())
  const enterTimers = new Map<string, number>()
  const motionVars = {
    '--entry-enter-duration': `${ENTRY_ENTER_ANIMATION_MS}ms`,
  }

  function scheduleEnterAnimation(id: string) {
    const currentTimer = enterTimers.get(id)
    if (currentTimer) window.clearTimeout(currentTimer)

    enteringIds.add(id)
    const timer = window.setTimeout(() => {
      enteringIds.delete(id)
      enterTimers.delete(id)
    }, ENTRY_ENTER_ANIMATION_MS)
    enterTimers.set(id, timer)
  }

  watch(
    () => store.transient.lastRealtimeAddedId,
    async (addedId) => {
      if (!addedId) return
      await nextTick()

      if (visibleEntryIds.value.includes(addedId)) {
        scheduleEnterAnimation(addedId)
      }
    },
  )

  onBeforeUnmount(() => {
    for (const timer of enterTimers.values()) {
      window.clearTimeout(timer)
    }
    enterTimers.clear()
  })

  return {
    enteringIds,
    motionVars,
  }
}
