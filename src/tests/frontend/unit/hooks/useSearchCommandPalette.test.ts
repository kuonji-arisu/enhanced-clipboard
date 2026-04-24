import { nextTick, ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import {
  createEntrySearchCommandFilters,
} from '../../../../utils/entrySearchCommands'
import { useSearchCommandPalette } from '../../../../hooks/useSearchCommandPalette'
import { installTestPinia, primeAppInfoStore } from '../../support/pinia'

describe('useSearchCommandPalette', () => {
  it('opens the command palette and applies the selected filter value', async () => {
    installTestPinia()
    primeAppInfoStore()

    const inputElement = document.createElement('input')
    inputElement.focus = vi.fn()
    inputElement.setSelectionRange = vi.fn()

    const searchInput = ref('')
    const searchCommandFilters = ref(createEntrySearchCommandFilters())
    const applyFilter = vi.fn()

    const palette = useSearchCommandPalette({
      inputRef: ref(inputElement),
      searchInput,
      searchCommandFilters,
      isCompositionKeydown: () => false,
      applyFilter,
      setSearchInput: (value) => {
        searchInput.value = value
      },
      setSearchCommandFilter: (command, value) => {
        searchCommandFilters.value = {
          ...searchCommandFilters.value,
          [command]: value,
        }
      },
      clearSearchCommandFilter: (command) => {
        searchCommandFilters.value = {
          ...searchCommandFilters.value,
          [command]: null,
        }
      },
    })

    palette.onFocus()
    palette.onInputKeydown(new KeyboardEvent('keydown', { key: '/' }))
    await nextTick()

    expect(palette.showCommandMenu.value).toBe(true)
    expect(palette.commandOptions.value[0]?.value).toBe('type')

    palette.selectCommand('type')
    await nextTick()
    expect(palette.commandMenuTitle.value.length).toBeGreaterThan(0)

    palette.selectCommand('text')
    await nextTick()

    expect(searchCommandFilters.value.type).toBe('text')
    expect(applyFilter).toHaveBeenCalledOnce()
  })

  it('falls back to literal slash input when slash is pressed inside the root palette', async () => {
    installTestPinia()
    primeAppInfoStore()

    const inputElement = document.createElement('input')
    inputElement.focus = vi.fn()
    inputElement.setSelectionRange = vi.fn()

    const searchInput = ref('')
    const palette = useSearchCommandPalette({
      inputRef: ref(inputElement),
      searchInput,
      searchCommandFilters: ref(createEntrySearchCommandFilters()),
      isCompositionKeydown: () => false,
      applyFilter: vi.fn(),
      setSearchInput: (value) => {
        searchInput.value = value
      },
      setSearchCommandFilter: vi.fn(),
      clearSearchCommandFilter: vi.fn(),
    })

    palette.onFocus()
    palette.onInputKeydown(new KeyboardEvent('keydown', { key: '/' }))
    palette.onInputKeydown(new KeyboardEvent('keydown', { key: '/' }))
    await nextTick()

    expect(searchInput.value).toBe('/')
    expect(palette.showCommandMenu.value).toBe(false)
  })
})
