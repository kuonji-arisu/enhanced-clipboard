import { useI18n, type I18nKey } from '../i18n'
import { useNoticeStore } from '../stores/notice'

type RunOptions = {
  titleKey?: I18nKey
  rethrow?: boolean
}

export function useAsyncAction() {
  const noticeStore = useNoticeStore()
  const { t } = useI18n()

  async function run<T>(
    action: () => Promise<T>,
    fallbackKey: I18nKey,
    options: RunOptions = {},
  ): Promise<T | undefined> {
    try {
      return await action()
    } catch (error) {
      noticeStore.openActionError(
        t(options.titleKey ?? 'actionErrorTitle'),
        error,
        t(fallbackKey),
      )
      if (options.rethrow) throw error
      return undefined
    }
  }

  return { run }
}
