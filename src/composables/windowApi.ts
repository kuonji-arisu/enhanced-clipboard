import { getCurrentWindow } from '@tauri-apps/api/window'

const currentWindow = getCurrentWindow()

export async function closeCurrentWindow(): Promise<void> {
  await currentWindow.close()
}
