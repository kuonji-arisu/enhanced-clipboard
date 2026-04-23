import { defineStore } from 'pinia'
import {
  clearAll,
  copyEntry,
  deleteEntry,
  reportImageLoadFailed,
  togglePin as togglePinEntry,
} from '../composables/clipboardApi'

export const useClipboardActionsStore = defineStore('clipboardActions', () => {
  async function copy(id: string) {
    await copyEntry(id)
  }

  async function remove(id: string) {
    await deleteEntry(id)
  }

  async function handleImageLoadFailed(id: string) {
    await reportImageLoadFailed(id)
  }

  async function clear() {
    await clearAll()
  }

  async function togglePin(id: string) {
    await togglePinEntry(id)
  }

  return {
    copy,
    remove,
    handleImageLoadFailed,
    clear,
    togglePin,
  }
})
