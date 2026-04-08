import { globalNow } from './useNow'
import { useI18n } from '../i18n'

/**
 * 基于全局 `now` 的响应式时间格式化（每秒自动刷新）。
 * 分级策略：
 *   < 1 min    → 刚刚 / Just now
 *   1–59 min   → N 分钟前 / N min ago
 *   今天 ≥ 1h  → HH:MM
 *   昨天        → 昨天 HH:MM / Yesterday HH:MM
 *   今年内      → M月D日 / MMM D
 *   更早        → YYYY年M月D日 / MMM D, YYYY
 */
export function useRelativeTime() {
  const { t, intlLocale, isZhLocale } = useI18n()

  /** 相对时间（依赖 globalNow，自动刷新） */
  function formatTime(createdAt: number): string {
    const now = globalNow.value
    const diffSec = now - createdAt

    if (diffSec < 60) return t('justNow')

    if (diffSec < 3600) return `${Math.floor(diffSec / 60)} ${t('minutesAgo')}`

    const date = new Date(createdAt * 1000)
    const nowDate = new Date(now * 1000)
    const hh = String(date.getHours()).padStart(2, '0')
    const mm = String(date.getMinutes()).padStart(2, '0')
    const timeStr = `${hh}:${mm}`

    const todayStart = new Date(nowDate.getFullYear(), nowDate.getMonth(), nowDate.getDate())
    const yesterdayStart = new Date(todayStart.getTime() - 86_400_000)

    if (date >= todayStart) return timeStr

    if (date >= yesterdayStart) return `${t('yesterday')} ${timeStr}`

    const month = date.getMonth() + 1
    const day = date.getDate()
    const year = date.getFullYear()

    if (year === nowDate.getFullYear()) {
      return isZhLocale.value
        ? `${month}月${day}日 ${timeStr}`
        : `${date.toLocaleDateString(intlLocale.value, { month: 'short', day: 'numeric' })} ${timeStr}`
    }

    return isZhLocale.value
      ? `${year}年${month}月${day}日 ${timeStr}`
      : `${date.toLocaleDateString(intlLocale.value, { month: 'short', day: 'numeric', year: 'numeric' })} ${timeStr}`
  }

  /** 完整时间戳字符串（用于 Tooltip） */
  function formatFull(createdAt: number): string {
    const d = new Date(createdAt * 1000)
    const yyyy = d.getFullYear()
    const MM = String(d.getMonth() + 1).padStart(2, '0')
    const dd = String(d.getDate()).padStart(2, '0')
    const hh = String(d.getHours()).padStart(2, '0')
    const mm = String(d.getMinutes()).padStart(2, '0')
    const ss = String(d.getSeconds()).padStart(2, '0')
    return `${yyyy}-${MM}-${dd} ${hh}:${mm}:${ss}`
  }

  return { formatTime, formatFull }
}
