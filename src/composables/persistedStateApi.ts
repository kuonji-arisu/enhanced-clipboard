import { invoke } from '@tauri-apps/api/core'
import type { PersistedState, PersistedStatePatch, SavePersistedResult } from '../types'

export async function fetchPersistedState(): Promise<PersistedState> {
  return invoke<PersistedState>('get_persisted')
}

export async function savePersistedState(patch: PersistedStatePatch): Promise<SavePersistedResult> {
  return invoke<SavePersistedResult>('save_persisted', { patch })
}
