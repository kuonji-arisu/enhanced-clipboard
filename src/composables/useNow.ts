/**
 * 全局时间驱动器 — 模块级单例。
 * 每秒更新一次 `globalNow`（epoch 秒），同时驱动：
 * - 相对时间显示刷新
 * - 前台过期条目移除
 *
 * 页面不可见时停止计时，可见时立即追赶并重启，避免后台无效唤醒。
 */
import { ref } from 'vue'

/** 当前时间（epoch 秒），每秒更新 */
export const globalNow = ref(Math.floor(Date.now() / 1000))

let _timerId: ReturnType<typeof setInterval> | null = null

function tick(): void {
  globalNow.value = Math.floor(Date.now() / 1000)
}

function startTimer(): void {
  if (_timerId !== null) return
  _timerId = setInterval(tick, 1000)
}

function stopTimer(): void {
  if (_timerId === null) return
  clearInterval(_timerId)
  _timerId = null
}

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    tick()        // 立即追赶休眠/最小化期间的时间跳跃
    startTimer()
  } else {
    stopTimer()
  }
})

startTimer()
