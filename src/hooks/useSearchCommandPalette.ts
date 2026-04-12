import { computed, nextTick, ref, watch, type Ref } from 'vue'
import { useI18n } from '../i18n'
import {
  getAppliedEntrySearchCommandFilterChips,
  getEntrySearchCommandSuggestions,
  getEntrySearchCommandTitleKey,
  getEntrySearchCommandValueSuggestions,
  type EntrySearchCommandFilterValue,
  type EntrySearchCommandFilters,
  type EntrySearchCommandValue,
} from '../utils/entrySearchCommands'

interface UseSearchCommandPaletteParams {
  inputRef: Ref<HTMLInputElement | null>
  searchInput: Ref<string>
  searchCommandFilters: Ref<EntrySearchCommandFilters>
  isCompositionKeydown: (event: KeyboardEvent) => boolean
  applyFilter: () => void
  setSearchInput: (value: string) => void
  setSearchCommandFilter: (
    command: EntrySearchCommandValue,
    value: EntrySearchCommandFilterValue | null,
  ) => void
  clearSearchCommandFilter: (command: EntrySearchCommandValue) => void
}

export function useSearchCommandPalette({
  inputRef,
  searchInput,
  searchCommandFilters,
  isCompositionKeydown,
  applyFilter,
  setSearchInput,
  setSearchCommandFilter,
  clearSearchCommandFilter,
}: UseSearchCommandPaletteParams) {
  const { t } = useI18n()

  const inputFocused = ref(false)
  const commandMenuOpen = ref(false)
  const activeCommand = ref<EntrySearchCommandValue | null>(null)
  const commandDraft = ref('')
  const highlightedCommandIndex = ref(0)

  const commandOptions = computed(() => {
    if (activeCommand.value === null) {
      return getEntrySearchCommandSuggestions(commandDraft.value).map((option) => ({
        value: option.value,
        token: option.token,
        label: t(option.labelKey),
      }))
    }

    const command = activeCommand.value
    return getEntrySearchCommandValueSuggestions(command, commandDraft.value).map((option) => ({
      value: option.value,
      token: option.token,
      label: t(option.labelKey),
    }))
  })

  const activeFilterChips = computed(() =>
    getAppliedEntrySearchCommandFilterChips(searchCommandFilters.value).map(({ key, labelKey }) => ({
      key,
      label: t(labelKey),
    })),
  )

  const showCommandMenu = computed(() =>
    inputFocused.value &&
    commandMenuOpen.value &&
    commandOptions.value.length > 0,
  )

  const commandMenuTitle = computed(() =>
    activeCommand.value === null
      ? t('searchCommandMenuTitle')
      : t(getEntrySearchCommandTitleKey(activeCommand.value)),
  )

  const activeCommandValue = computed(() =>
    showCommandMenu.value ? commandOptions.value[highlightedCommandIndex.value] ?? null : null,
  )

  function openCommandMenu(command: EntrySearchCommandValue | null = null) {
    commandMenuOpen.value = true
    activeCommand.value = command
    commandDraft.value = ''
    highlightedCommandIndex.value = 0
    inputRef.value?.focus()
  }

  function closeCommandMenu() {
    commandMenuOpen.value = false
    activeCommand.value = null
    commandDraft.value = ''
    highlightedCommandIndex.value = 0
  }

  function appendCommandDraft(key: string) {
    commandDraft.value += key.toLowerCase()
    highlightedCommandIndex.value = 0
  }

  function trimCommandDraft() {
    commandDraft.value = commandDraft.value.slice(0, -1)
    highlightedCommandIndex.value = 0
  }

  function isPrintableKey(event: KeyboardEvent): boolean {
    return event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey
  }

  function insertSearchText(text: string) {
    const inputEl = inputRef.value
    const currentValue = searchInput.value
    const selectionStart = inputEl?.selectionStart ?? currentValue.length
    const selectionEnd = inputEl?.selectionEnd ?? selectionStart
    const nextValue =
      currentValue.slice(0, selectionStart) +
      text +
      currentValue.slice(selectionEnd)
    const nextCaret = selectionStart + text.length

    setSearchInput(nextValue)
    applyFilter()

    void nextTick(() => {
      inputEl?.focus()
      inputEl?.setSelectionRange(nextCaret, nextCaret)
    })
  }

  function recoverCommandInput(text: string) {
    closeCommandMenu()
    insertSearchText(text)
  }

  function onFocus() {
    inputFocused.value = true
  }

  function onBlur() {
    inputFocused.value = false
    closeCommandMenu()
  }

  function selectCommand(value: string) {
    if (activeCommand.value === null) {
      openCommandMenu(value as EntrySearchCommandValue)
      return
    }

    setSearchCommandFilter(activeCommand.value, value as EntrySearchCommandFilterValue)
    closeCommandMenu()
    applyFilter()
  }

  function clearFilter(command: EntrySearchCommandValue) {
    clearSearchCommandFilter(command)
    applyFilter()
  }

  function onInputKeydown(event: KeyboardEvent) {
    if (isCompositionKeydown(event)) {
      return
    }

    if (commandMenuOpen.value) {
      if (event.key === '/') {
        event.preventDefault()
        recoverCommandInput(activeCommand.value === null ? '/' : event.key)
        return
      }

      if (event.key === 'ArrowDown' && commandOptions.value.length > 0) {
        event.preventDefault()
        highlightedCommandIndex.value =
          (highlightedCommandIndex.value + 1) % commandOptions.value.length
        return
      }

      if (event.key === 'ArrowUp' && commandOptions.value.length > 0) {
        event.preventDefault()
        highlightedCommandIndex.value =
          (highlightedCommandIndex.value - 1 + commandOptions.value.length) % commandOptions.value.length
        return
      }

      if (event.key === 'Tab' && commandOptions.value.length > 0) {
        event.preventDefault()
        const step = event.shiftKey ? -1 : 1
        highlightedCommandIndex.value =
          (highlightedCommandIndex.value + step + commandOptions.value.length) % commandOptions.value.length
        return
      }

      if (event.key === 'Enter') {
        const next = activeCommandValue.value
        if (!next) return
        event.preventDefault()
        selectCommand(next.value)
        return
      }

      if (event.key === 'Escape') {
        event.preventDefault()
        closeCommandMenu()
        return
      }

      if (event.key === 'Backspace') {
        event.preventDefault()
        if (commandDraft.value) {
          trimCommandDraft()
          return
        }

        if (activeCommand.value !== null) {
          openCommandMenu()
          return
        }

        closeCommandMenu()
        return
      }

      if (isPrintableKey(event) && event.key !== '/') {
        const nextDraft = `${commandDraft.value}${event.key.toLowerCase()}`
        const nextOptions = activeCommand.value === null
          ? getEntrySearchCommandSuggestions(nextDraft)
          : getEntrySearchCommandValueSuggestions(activeCommand.value, nextDraft)

        if (nextOptions.length > 0) {
          event.preventDefault()
          appendCommandDraft(event.key)
          return
        }

        event.preventDefault()
        recoverCommandInput(activeCommand.value === null ? `/${event.key}` : event.key)
        return
      }

      return
    }

    if (event.key === '/' && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault()
      openCommandMenu()
      return
    }

    if (event.key === 'Backspace' && !searchInput.value && activeFilterChips.value.length > 0) {
      event.preventDefault()
      clearFilter(activeFilterChips.value[activeFilterChips.value.length - 1].key)
    }
  }

  watch(commandOptions, (options) => {
    if (commandMenuOpen.value && options.length === 0) {
      closeCommandMenu()
      return
    }

    if (options.length === 0) {
      highlightedCommandIndex.value = 0
      return
    }

    if (highlightedCommandIndex.value >= options.length) {
      highlightedCommandIndex.value = 0
    }
  })

  return {
    activeFilterChips,
    showCommandMenu,
    commandMenuTitle,
    commandDraft,
    commandOptions,
    activeCommandValue,
    onFocus,
    onBlur,
    selectCommand,
    clearFilter,
    onInputKeydown,
  }
}
