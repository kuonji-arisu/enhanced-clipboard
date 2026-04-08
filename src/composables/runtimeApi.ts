import { invoke } from '@tauri-apps/api/core'
import type { RuntimeStatus } from '../types'

export async function fetchRuntimeStatus(): Promise<RuntimeStatus> {
  return invoke<RuntimeStatus>('get_runtime_status')
}
