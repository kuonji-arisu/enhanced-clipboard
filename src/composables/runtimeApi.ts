import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { EVENT_RUNTIME_STATUS_UPDATED } from '../constants'
import type { RuntimeStatus, RuntimeStatusPatch } from '../types'

export type RuntimeStatusUnlisten = UnlistenFn

export async function fetchRuntimeStatus(): Promise<RuntimeStatus> {
  return invoke<RuntimeStatus>('get_runtime_status')
}

export async function listenRuntimeStatusPatches(
  handler: (patch: RuntimeStatusPatch) => void,
): Promise<RuntimeStatusUnlisten> {
  return listen<RuntimeStatusPatch>(EVENT_RUNTIME_STATUS_UPDATED, (event) => {
    handler(event.payload)
  })
}
