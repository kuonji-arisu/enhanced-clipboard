import { ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'

// 模块级单例：整个应用共享同一个置顶状态，跨路由切换不会重置
const pinned = ref(false)
const win = getCurrentWindow()

export function usePinState() {
  async function togglePin() {
    pinned.value = !pinned.value
    await win.setAlwaysOnTop(pinned.value)
  }

  return { pinned, togglePin }
}
