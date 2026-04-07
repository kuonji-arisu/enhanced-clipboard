import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getErrorMessage } from '../utils/errors'

export const useNoticeStore = defineStore('notice', () => {
  const showDialog = ref(false)
  const dialogTitle = ref('')
  const dialogMessage = ref('')
  const clipboardCaptureAvailable = ref(true)

  function openError(title: string, message: string) {
    dialogTitle.value = title
    dialogMessage.value = message
    showDialog.value = true
  }

  function closeDialog() {
    showDialog.value = false
  }

  function openActionError(title: string, error: unknown, fallback: string) {
    openError(title, getErrorMessage(error, fallback))
  }

  function setCaptureRuntimeStatus(
    available: boolean,
    title?: string,
    message?: string,
  ) {
    const changedToUnavailable = clipboardCaptureAvailable.value && !available
    clipboardCaptureAvailable.value = available
    if (changedToUnavailable && title && message) {
      openError(title, message)
    }
  }

  return {
    showDialog,
    dialogTitle,
    dialogMessage,
    clipboardCaptureAvailable,
    openError,
    openActionError,
    closeDialog,
    setCaptureRuntimeStatus,
  }
})
