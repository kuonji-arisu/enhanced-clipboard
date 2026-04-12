import type { I18nKey } from '../i18n'

export type EntrySearchTypeValue = 'text' | 'image'
export type EntrySearchCommandFilterValue = string

export interface EntrySearchFilters {
  text: string
  entryType: EntrySearchTypeValue | null
}

export interface EntrySearchCommandOptionDefinition<Value extends string = string> {
  value: Value
  labelKey: I18nKey
}

interface EntrySearchCommandDefinition {
  titleKey: I18nKey
  descriptionKey: I18nKey
  valueOptions: readonly EntrySearchCommandOptionDefinition[]
  applyToSearchFilters(
    filters: EntrySearchFilters,
    value: string,
  ): EntrySearchFilters
}

const ENTRY_SEARCH_COMMAND_DEFINITIONS = {
  type: {
    titleKey: 'searchCommandTypeLabel',
    descriptionKey: 'searchCommandTypeDescription',
    valueOptions: [
      { value: 'text', labelKey: 'searchTypeText' },
      { value: 'image', labelKey: 'searchTypeImage' },
    ],
    applyToSearchFilters(filters, value) {
      return {
        ...filters,
        entryType: value as EntrySearchTypeValue,
      }
    },
  },
} as const satisfies Record<string, EntrySearchCommandDefinition>

export type EntrySearchCommandValue = Extract<keyof typeof ENTRY_SEARCH_COMMAND_DEFINITIONS, string>
export type EntrySearchCommandFilters =
  Record<EntrySearchCommandValue, EntrySearchCommandFilterValue | null>

export interface EntrySearchCommandMenuOption<T extends string = string> {
  value: T
  token: string
  labelKey: I18nKey
}

function getEntrySearchCommandKeys(): EntrySearchCommandValue[] {
  return Object.keys(ENTRY_SEARCH_COMMAND_DEFINITIONS) as EntrySearchCommandValue[]
}

export function createEntrySearchCommandFilters(): EntrySearchCommandFilters {
  return getEntrySearchCommandKeys().reduce((filters, command) => {
    filters[command] = null
    return filters
  }, {} as EntrySearchCommandFilters)
}

function getEntrySearchCommandDefinition(
  command: EntrySearchCommandValue,
): EntrySearchCommandDefinition {
  return ENTRY_SEARCH_COMMAND_DEFINITIONS[command]
}

export function getEntrySearchCommandTitleKey(
  command: EntrySearchCommandValue,
): I18nKey {
  return getEntrySearchCommandDefinition(command).titleKey
}

function getEntrySearchCommandFilter(
  filters: EntrySearchCommandFilters,
  command: EntrySearchCommandValue,
): EntrySearchCommandFilterValue | null {
  return filters[command]
}

export function setEntrySearchCommandFilter(
  filters: EntrySearchCommandFilters,
  command: EntrySearchCommandValue,
  value: EntrySearchCommandFilterValue | null,
): EntrySearchCommandFilters {
  return {
    ...filters,
    [command]: value,
  }
}

function getAppliedEntrySearchCommandFilters(
  filters: EntrySearchCommandFilters,
): Array<{ key: EntrySearchCommandValue, value: EntrySearchCommandFilterValue }> {
  return getEntrySearchCommandKeys().flatMap((command) => {
    const value = getEntrySearchCommandFilter(filters, command)
    return value ? [{ key: command, value }] : []
  })
}

export function getAppliedEntrySearchCommandFilterChips(
  filters: EntrySearchCommandFilters,
): Array<{ key: EntrySearchCommandValue, labelKey: I18nKey }> {
  return getAppliedEntrySearchCommandFilters(filters).map(({ key, value }) => ({
    key,
    labelKey: getEntrySearchCommandValueLabelKey(key, value),
  }))
}

function getEntrySearchCommandValueLabelKey(
  command: EntrySearchCommandValue,
  value: EntrySearchCommandFilterValue,
): I18nKey {
  return getEntrySearchCommandDefinition(command).valueOptions.find((option) => option.value === value)?.labelKey
    ?? getEntrySearchCommandDefinition(command).titleKey
}

export function buildEntrySearchFilters(
  input: string,
  commandFilters: EntrySearchCommandFilters,
): EntrySearchFilters {
  const initialFilters: EntrySearchFilters = {
    text: input.trim(),
    entryType: null,
  }

  return getEntrySearchCommandKeys().reduce<EntrySearchFilters>((filters, command) => {
    const value = getEntrySearchCommandFilter(commandFilters, command)
    if (!value) return filters
    return getEntrySearchCommandDefinition(command).applyToSearchFilters(filters, value)
  }, initialFilters)
}

export function getEntrySearchCommandSuggestions(
  draft: string,
): EntrySearchCommandMenuOption<EntrySearchCommandValue>[] {
  const normalizedDraft = draft.trim().toLowerCase()
  return getEntrySearchCommandKeys()
    .filter((command) => command.startsWith(normalizedDraft))
    .map((command) => ({
      value: command,
      token: `/${command}`,
      labelKey: getEntrySearchCommandDefinition(command).descriptionKey,
    }))
}

export function getEntrySearchCommandValueSuggestions(
  command: EntrySearchCommandValue,
  draft: string,
): EntrySearchCommandMenuOption<EntrySearchCommandFilterValue>[] {
  const normalizedDraft = draft.trim().toLowerCase()
  return getEntrySearchCommandDefinition(command).valueOptions
    .filter((option) => option.value.startsWith(normalizedDraft))
    .map((option) => ({
      value: option.value,
      token: option.value,
      labelKey: option.labelKey,
    }))
}
