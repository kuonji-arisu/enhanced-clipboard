import { invoke } from '@tauri-apps/api/core'
import type { PersistedState } from '../types'

export async function fetchPersistedState(): Promise<PersistedState> {
  return invoke<PersistedState>('get_persisted_state')
}

export async function setAlwaysOnTop(enabled: boolean): Promise<void> {
  return invoke('set_always_on_top', { enabled })
}
