export const ENTRY_SEARCH_TYPE_OPTIONS = ['text', 'image'] as const

export type EntrySearchTypeValue = (typeof ENTRY_SEARCH_TYPE_OPTIONS)[number]

export interface EntrySearchFilters {
  text: string
  entryType: EntrySearchTypeValue | null
}

export interface SearchTokenRange {
  start: number
  end: number
}

export interface ActiveSearchToken {
  activeTokenText: string
  activeTokenRange: SearchTokenRange | null
}

interface IndexedToken {
  text: string
  range: SearchTokenRange
}

function tokenizeInput(input: string): IndexedToken[] {
  const tokens: IndexedToken[] = []
  const pattern = /\S+/g

  for (const match of input.matchAll(pattern)) {
    const text = match[0]
    const start = match.index ?? 0
    tokens.push({
      text,
      range: {
        start,
        end: start + text.length,
      },
    })
  }

  return tokens
}

function resolveActiveToken(tokens: IndexedToken[], cursor: number): IndexedToken | null {
  for (const token of tokens) {
    if (cursor >= token.range.start && cursor <= token.range.end) {
      return token
    }
  }
  return null
}

export function getActiveSearchToken(
  input: string,
  cursor = input.length,
): ActiveSearchToken {
  const tokens = tokenizeInput(input)
  const activeToken = resolveActiveToken(tokens, cursor)

  return {
    activeTokenText: activeToken?.text ?? '',
    activeTokenRange: activeToken?.range ?? null,
  }
}

export function buildEntrySearchFilters(
  input: string,
  entryType: EntrySearchTypeValue | null,
): EntrySearchFilters {
  return {
    text: input.trim(),
    entryType,
  }
}

export function getEntrySearchTypeSuggestions(draft: string): EntrySearchTypeValue[] {
  const normalizedDraft = draft.trim().toLowerCase()
  return ENTRY_SEARCH_TYPE_OPTIONS.filter((option) => option.startsWith(normalizedDraft))
}

export function removeSearchToken(
  input: string,
  range: SearchTokenRange,
): { value: string, caret: number } {
  const left = input.slice(0, range.start)
  const right = input.slice(range.end)
  const nextValue = [left.trimEnd(), right.trimStart()].filter(Boolean).join(' ')
  const caret = left.trimEnd().length

  return {
    value: nextValue,
    caret,
  }
}
