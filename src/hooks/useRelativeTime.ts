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
  const { t, intlLocale } = useI18n()

  /** 相对时间（依赖 globalNow，自动刷新） */
  function formatTime(createdAt: number): string {
    const now = globalNow.value
    const diffSec = now - createdAt

    if (diffSec < 60) return t('justNow')

    if (diffSec < 3600) {
      return t('relativeMinutesAgo', { count: Math.floor(diffSec / 60) })
    }

    const date = new Date(createdAt * 1000)
    const nowDate = new Date(now * 1000)
    const timeFormatter = new Intl.DateTimeFormat(intlLocale.value, {
      hour: '2-digit',
      minute: '2-digit',
      hourCycle: 'h23',
    })
    const sameYearFormatter = new Intl.DateTimeFormat(intlLocale.value, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hourCycle: 'h23',
    })
    const olderFormatter = new Intl.DateTimeFormat(intlLocale.value, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hourCycle: 'h23',
    })
    const timeStr = timeFormatter.format(date)

    const todayStart = new Date(nowDate.getFullYear(), nowDate.getMonth(), nowDate.getDate())
    const yesterdayStart = new Date(todayStart.getTime() - 86_400_000)

    if (date >= todayStart) return timeStr

    if (date >= yesterdayStart) return t('relativeYesterdayAt', { time: timeStr })

    if (date.getFullYear() === nowDate.getFullYear()) return sameYearFormatter.format(date)

    return olderFormatter.format(date)
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
