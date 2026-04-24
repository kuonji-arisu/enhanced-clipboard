import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useCalendarMetaStore } from '../../../../stores/calendarMeta'
import { useClipboardStreamStore } from '../../../../stores/clipboardStream'
import {
  beginSettingsSaveVisibilitySession,
  cancelSettingsSaveVisibilitySession,
  finishSettingsSaveVisibilitySession,
  handleSettingsDrivenVisibilityStale,
} from '../../../../utils/clipboardViewCoordinator'
import { installTestPinia, primeAppInfoStore, primeSettingsStore } from '../../support/pinia'

describe('clipboardViewCoordinator', () => {
  beforeEach(() => {
    installTestPinia()
    primeAppInfoStore()
    primeSettingsStore()
  })

  it('reconciles visibility immediately when no save session is active', async () => {
    const streamStore = useClipboardStreamStore()
    const calendarStore = useCalendarMetaStore()
    const loadInitial = vi.spyOn(streamStore, 'loadInitial').mockResolvedValue()
    const refreshCalendarMeta = vi.spyOn(calendarStore, 'refreshCalendarMeta').mockResolvedValue()

    await handleSettingsDrivenVisibilityStale()

    expect(loadInitial).toHaveBeenCalledOnce()
    expect(refreshCalendarMeta).toHaveBeenCalledOnce()
  })

  it('queues reconciliation until the active save session finishes', async () => {
    const streamStore = useClipboardStreamStore()
    const calendarStore = useCalendarMetaStore()
    const loadInitial = vi.spyOn(streamStore, 'loadInitial').mockResolvedValue()
    const refreshCalendarMeta = vi.spyOn(calendarStore, 'refreshCalendarMeta').mockResolvedValue()

    beginSettingsSaveVisibilitySession()
    await handleSettingsDrivenVisibilityStale()

    expect(loadInitial).not.toHaveBeenCalled()
    expect(refreshCalendarMeta).not.toHaveBeenCalled()

    await finishSettingsSaveVisibilitySession()

    expect(loadInitial).toHaveBeenCalledOnce()
    expect(refreshCalendarMeta).toHaveBeenCalledOnce()
  })

  it('waits for all overlapping save sessions and drops reconcile on cancel', async () => {
    const streamStore = useClipboardStreamStore()
    const calendarStore = useCalendarMetaStore()
    const loadInitial = vi.spyOn(streamStore, 'loadInitial').mockResolvedValue()
    const refreshCalendarMeta = vi.spyOn(calendarStore, 'refreshCalendarMeta').mockResolvedValue()

    beginSettingsSaveVisibilitySession()
    beginSettingsSaveVisibilitySession()
    await handleSettingsDrivenVisibilityStale()

    await finishSettingsSaveVisibilitySession()
    expect(loadInitial).not.toHaveBeenCalled()
    expect(refreshCalendarMeta).not.toHaveBeenCalled()

    cancelSettingsSaveVisibilitySession()
    expect(loadInitial).not.toHaveBeenCalled()
    expect(refreshCalendarMeta).not.toHaveBeenCalled()

    await handleSettingsDrivenVisibilityStale()
    expect(loadInitial).toHaveBeenCalledOnce()
    expect(refreshCalendarMeta).toHaveBeenCalledOnce()
  })
})
