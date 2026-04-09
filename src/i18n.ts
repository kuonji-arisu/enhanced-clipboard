import { computed } from 'vue'
import { useAppInfoStore } from './stores/appInfo'
import enUS from '../i18n/en-US.json'

type MessageSchema = Record<string, string>
type I18nParams = Record<string, string | number>

const DEFAULT_LOCALE = 'en-US'
const messageModules = import.meta.glob<MessageSchema>('../i18n/*.json', {
  eager: true,
  import: 'default',
})
const messages = Object.fromEntries(
  Object.entries(messageModules).map(([path, message]) => {
    const match = path.match(/\/([^/]+)\.json$/)
    return [match?.[1] ?? DEFAULT_LOCALE, message]
  }),
) as Record<string, MessageSchema>

export type I18nKey = keyof typeof enUS

function normalizeLocaleTag(locale: string): string {
  return locale.trim().replace(/_/g, '-').toLowerCase()
}

function findBestLocaleMatch(locale: string | null | undefined): string {
  const normalized = normalizeLocaleTag(locale ?? '')
  const availableLocales = Object.keys(messages)

  const exactMatch = availableLocales.find((item) => normalizeLocaleTag(item) === normalized)
  if (exactMatch) return exactMatch

  return DEFAULT_LOCALE
}

export function toIntlLocale(locale: string): string {
  return locale.replace(/_/g, '-')
}

export function useI18n() {
  const appInfoStore = useAppInfoStore()
  const activeLocale = computed<string>(() =>
    findBestLocaleMatch(appInfoStore.appInfo?.locale ?? navigator.language),
  )
  const intlLocale = computed(() => toIntlLocale(activeLocale.value))

  function t(key: I18nKey, params?: I18nParams): string {
    const template = messages[activeLocale.value]?.[key] ?? messages[DEFAULT_LOCALE]?.[key] ?? key
    if (!params) return template
    return template.replace(/\{(\w+)\}/g, (_, name: string) => {
      const value = params[name]
      return value === undefined ? `{${name}}` : String(value)
    })
  }

  return { t, activeLocale, intlLocale }
}
