import { invoke } from '@tauri-apps/api/core'
import type { AppSettings, AppSettingsPatch } from '../types'

export async function fetchSettings(): Promise<AppSettings> {
  return invoke<AppSettings>('get_settings')
}

export async function saveSettings(patch: AppSettingsPatch): Promise<void> {
  return invoke('save_settings', { patch })
}

export async function pauseHotkey(): Promise<void> {
  return invoke('pause_hotkey')
}

export async function resumeHotkey(): Promise<void> {
  return invoke('resume_hotkey')
}
