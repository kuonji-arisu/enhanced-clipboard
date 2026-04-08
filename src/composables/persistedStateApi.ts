import { invoke } from '@tauri-apps/api/core'
import type { PersistedState, PersistedStatePatch } from '../types'

export async function fetchPersistedState(): Promise<PersistedState> {
  return invoke<PersistedState>('get_persisted_state')
}

export async function savePersistedState(patch: PersistedStatePatch): Promise<void> {
  return invoke('save_persisted_state', { patch })
}
