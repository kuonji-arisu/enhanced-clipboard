import { useCalendarMetaStore } from '../stores/calendarMeta'
import { useClipboardStreamStore } from '../stores/clipboardStream'

let settingsSaveSessionCount = 0
let pendingSettingsVisibilityReconcile = false
let reconcileQueue: Promise<void> = Promise.resolve()

async function reconcileSettingsVisibility(): Promise<void> {
  const calendarMetaStore = useCalendarMetaStore()
  const streamStore = useClipboardStreamStore()

  await streamStore.loadInitial()
  await calendarMetaStore.refreshCalendarMeta()
}

function enqueueReconcile(): Promise<void> {
  reconcileQueue = reconcileQueue
    .catch(() => undefined)
    .then(() => reconcileSettingsVisibility())
  return reconcileQueue
}

export function beginSettingsSaveVisibilitySession(): void {
  settingsSaveSessionCount += 1
}

export function cancelSettingsSaveVisibilitySession(): void {
  settingsSaveSessionCount = Math.max(0, settingsSaveSessionCount - 1)
  if (settingsSaveSessionCount === 0) {
    pendingSettingsVisibilityReconcile = false
  }
}

export async function finishSettingsSaveVisibilitySession(): Promise<void> {
  settingsSaveSessionCount = Math.max(0, settingsSaveSessionCount - 1)
  if (settingsSaveSessionCount > 0 || !pendingSettingsVisibilityReconcile) {
    return
  }

  pendingSettingsVisibilityReconcile = false
  await enqueueReconcile()
}

export async function handleSettingsDrivenVisibilityStale(): Promise<void> {
  if (settingsSaveSessionCount > 0) {
    pendingSettingsVisibilityReconcile = true
    return
  }

  await enqueueReconcile()
}
