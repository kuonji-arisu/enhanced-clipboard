import { describe, expect, it } from 'vitest'
import {
  buildEntrySearchFilters,
  createEntrySearchCommandFilters,
  getAppliedEntrySearchCommandFilterChips,
  getEntrySearchCommandSuggestions,
  getEntrySearchCommandValueSuggestions,
  setEntrySearchCommandFilter,
} from '../../../../utils/entrySearchCommands'

describe('entrySearchCommands', () => {
  it('builds normalized filters from text and command chips', () => {
    let filters = createEntrySearchCommandFilters()
    filters = setEntrySearchCommandFilter(filters, 'type', 'image')
    filters = setEntrySearchCommandFilter(filters, 'tag', 'url')

    expect(buildEntrySearchFilters('  alpha beta  ', filters)).toEqual({
      text: 'alpha beta',
      entryType: 'image',
      tag: 'url',
    })
  })

  it('exposes applied command chips using shared label keys', () => {
    let filters = createEntrySearchCommandFilters()
    filters = setEntrySearchCommandFilter(filters, 'type', 'text')

    expect(getAppliedEntrySearchCommandFilterChips(filters)).toEqual([
      {
        key: 'type',
        labelKey: 'searchTypeText',
      },
    ])
  })

  it('filters command and value suggestions by the current draft', () => {
    expect(getEntrySearchCommandSuggestions('t')).toEqual([
      {
        value: 'type',
        token: '/type',
        labelKey: 'searchCommandTypeDescription',
      },
      {
        value: 'tag',
        token: '/tag',
        labelKey: 'searchCommandTagDescription',
      },
    ])

    expect(getEntrySearchCommandValueSuggestions('type', 'im')).toEqual([
      {
        value: 'image',
        token: 'image',
        labelKey: 'searchTypeImage',
      },
    ])
  })
})
