import { computed, inject, provide, type InjectionKey, type Ref } from 'vue'
import { useSettingsStore } from './stores/settings'
import zh from '../i18n/zh.json'
import en from '../i18n/en.json'

type Locale = 'zh' | 'en'

const messages = { zh, en } as const
const I18N_LANGUAGE_OVERRIDE_KEY: InjectionKey<Readonly<Ref<Locale | undefined>>> = Symbol('i18n-language-override')

export type I18nKey = keyof typeof zh

export function resolveLocale(preferredLanguage: string, systemLocale: string): Locale {
  if (preferredLanguage === 'zh' || preferredLanguage === 'en') return preferredLanguage
  return systemLocale.startsWith('zh') ? 'zh' : 'en'
}

export function provideI18nLanguageOverride(language: Readonly<Ref<Locale | undefined>>) {
  provide(I18N_LANGUAGE_OVERRIDE_KEY, language)
}

export function useI18n() {
  const store = useSettingsStore()
  const injectedLanguage = inject(I18N_LANGUAGE_OVERRIDE_KEY, undefined)
  const activeLanguage = computed<Locale>(() => injectedLanguage?.value ?? store.effectiveLang)

  function t(key: I18nKey): string {
    const lang = activeLanguage.value
    return (lang in messages ? messages[lang] : messages.en)[key]
  }
  return { t }
}
