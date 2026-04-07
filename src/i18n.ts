import { useSettingsStore } from './stores/settings'
import zh from '../i18n/zh.json'
import en from '../i18n/en.json'

type Locale = 'zh' | 'en'

const messages = { zh, en } as const

export type I18nKey = keyof typeof zh

export function useI18n() {
  const store = useSettingsStore()
  function t(key: I18nKey): string {
    const lang = store.effectiveLang as Locale
    return (lang in messages ? messages[lang] : messages.en)[key]
  }
  return { t }
}
