/**
 * 前端侧唯一的常量声明来源。
 * 各模块通过 import { ... } from '../constants' 引入。
 */

import type { I18nKey } from './i18n'

// ── 分页 ──────────────────────────────────────────────────────────────────────

/** 每页加载条目数 */
export const PAGE_SIZE = 50

/** 触发预加载时距末尾的条目数 */
export const LOAD_MORE_THRESHOLD = 8

// ── 历史记录数范围（与后端设置校验保持一致） ──────────────────────────────────

/** 最大历史记录数下限 */
export const MIN_HISTORY = 10

/** 最大历史记录数上限 */
export const MAX_HISTORY = 10000;

// ── UI 反馈延迟（毫秒） ───────────────────────────────────────────────────────

/** 复制成功标记的消失延迟 */
export const COPY_FEEDBACK_MS = 1500

/** 置顶错误提示的消失延迟 */
export const PIN_ERROR_MS = 2000

/** 最大置顶数（与后端 MAX_PINNED_ENTRIES 保持一致） */
export const MAX_PINNED = 3

// ── 虚拟列表参数 ──────────────────────────────────────────────────────────────

/** 条目行高估算值（px）：文字约 80px，图片约 130px，取中间值 */
export const VIRTUAL_ITEM_ESTIMATE_SIZE = 90

/** 虚拟列表 gap（px），等同于 CSS --space-2 */
export const VIRTUAL_LIST_GAP = 8

/** 虚拟列表上下内边距（px），等同于 CSS --space-3 */
export const VIRTUAL_LIST_PADDING = 12

/** 虚拟列表前后额外渲染的条目数，防止快速滚动时白屏 */
export const VIRTUAL_LIST_OVERSCAN = 5

// ── 自动过期选项 ────────────────────────────────────────────────

/** 自动过期选项列表，秒数 0 表示永不过期 */
export const EXPIRY_OPTIONS: Array<
  | { seconds: 0; count?: undefined; unit?: undefined }
  | { seconds: number; count: number; unit: I18nKey }
> = [
  { seconds: 0 },
  { seconds: 10 * 60,          count: 10, unit: 'minute' },
  { seconds: 30 * 60,          count: 30, unit: 'minute' },
  { seconds: 60 * 60,          count: 1,  unit: 'hour'   },
  { seconds: 24 * 60 * 60,     count: 1,  unit: 'day'    },
  { seconds: 7 * 24 * 60 * 60, count: 1,  unit: 'week'   },
]
