import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getErrorMessage } from '../utils/errors'

export const useNoticeStore = defineStore('notice', () => {
  const showDialog = ref(false)
  const dialogTitle = ref('')
  const dialogMessage = ref('')

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

  return {
    showDialog,
    dialogTitle,
    dialogMessage,
    openError,
    openActionError,
    closeDialog,
  }
})
