import { computed } from 'vue'
import { usePersistedStateStore } from '../stores/persistedState'
import { useAsyncAction } from './useAsyncAction'

export function usePinState() {
  const persistedStateStore = usePersistedStateStore()
  const { run } = useAsyncAction()
  const pinned = computed(() => persistedStateStore.persistedState.always_on_top)

  async function togglePin() {
    await run(() => persistedStateStore.toggleAlwaysOnTop(), 'pinWindowFailed')
  }

  return { pinned, togglePin }
}
