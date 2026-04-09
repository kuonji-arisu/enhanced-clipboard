import { computed } from 'vue'
import { useI18n } from '../i18n'
import { useNoticeStore } from '../stores/notice'
import { usePersistedStateStore } from '../stores/persistedState'

export function usePinState() {
  const persistedStateStore = usePersistedStateStore()
  const noticeStore = useNoticeStore()
  const { t } = useI18n()
  const pinned = computed(() => persistedStateStore.persisted.always_on_top)

  async function togglePin() {
    try {
      const result = await persistedStateStore.toggleAlwaysOnTop()
      const error = result.effects?.always_on_top?.error
      if (error) {
        noticeStore.openError(t('pinWindowWarningTitle'), error)
      }
    } catch (error) {
      noticeStore.openActionError(t('actionErrorTitle'), error, t('pinWindowFailed'))
    }
  }

  return { pinned, togglePin }
}
