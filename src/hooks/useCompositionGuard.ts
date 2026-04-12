import { ref } from 'vue'

export function useCompositionGuard() {
  const isComposing = ref(false)
  const skipNextInputApply = ref(false)

  function onCompositionStart() {
    isComposing.value = true
  }

  function onCompositionEnd() {
    isComposing.value = false
    skipNextInputApply.value = true
  }

  function shouldSkipInputApply() {
    if (isComposing.value) {
      return true
    }

    if (skipNextInputApply.value) {
      skipNextInputApply.value = false
      return true
    }

    return false
  }

  function isCompositionKeydown(event: KeyboardEvent) {
    return isComposing.value || event.isComposing
  }

  return {
    isComposing,
    onCompositionStart,
    onCompositionEnd,
    shouldSkipInputApply,
    isCompositionKeydown,
  }
}
