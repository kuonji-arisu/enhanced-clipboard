import type { I18nKey } from '../i18n'

export const ENTRY_TAG_VALUES = ['json', 'url', 'email'] as const

export type EntryTagValue = typeof ENTRY_TAG_VALUES[number]

interface EntryTagDefinition {
  labelKey: I18nKey
}

const ENTRY_TAG_DEFINITIONS: Record<EntryTagValue, EntryTagDefinition> = {
  json: { labelKey: 'entryTagJson' },
  url: { labelKey: 'entryTagUrl' },
  email: { labelKey: 'entryTagEmail' },
}

export const ENTRY_TAG_OPTIONS = ENTRY_TAG_VALUES.map((value) => ({
  value,
  labelKey: ENTRY_TAG_DEFINITIONS[value].labelKey,
}))

export function isEntryTagValue(value: string): value is EntryTagValue {
  return ENTRY_TAG_VALUES.includes(value as EntryTagValue)
}

export function getEntryTagLabelKey(tag: EntryTagValue): I18nKey {
  return ENTRY_TAG_DEFINITIONS[tag].labelKey
}
