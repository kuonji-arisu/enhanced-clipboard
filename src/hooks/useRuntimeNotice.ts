import { watch } from 'vue'
import { useI18n } from '../i18n'
import { useNoticeStore } from '../stores/notice'
import { useRuntimeStore } from '../stores/runtime'

export function useRuntimeNotice() {
  const runtimeStore = useRuntimeStore()
  const noticeStore = useNoticeStore()
  const { t } = useI18n()

  watch(() => runtimeStore.runtime.clipboard_capture_available, (available, previousAvailable) => {
    const changedToUnavailable = previousAvailable !== false && !available
    if (changedToUnavailable) {
      noticeStore.openError(
        t('captureUnavailableTitle'),
        t('captureUnavailableMessage'),
      )
    }
  }, { immediate: true })
}
