/**
 * 前端本地 UI 常量。
 * 仅保留与交互和渲染相关、无需后端裁决的值。
 */

/** 触发预加载时距末尾的条目数 */
export const LOAD_MORE_THRESHOLD = 8

// ── UI 反馈延迟（毫秒） ───────────────────────────────────────────────────────

/** 复制成功标记的消失延迟 */
export const COPY_FEEDBACK_MS = 1500

/** 置顶错误提示的消失延迟 */
export const PIN_ERROR_MS = 2000

/** 新条目淡入动画时长 */
export const ENTRY_ENTER_ANIMATION_MS = 180

/** 手动删除条目的淡出动画时长 */
export const ENTRY_EXIT_ANIMATION_MS = 140

/** 置顶状态反馈动画时长 */
export const ENTRY_PIN_FEEDBACK_MS = 220

// ── 虚拟列表参数 ──────────────────────────────────────────────────────────────

/** 条目行高估算值（px）：文字约 80px，图片约 130px，取中间值 */
export const VIRTUAL_ITEM_ESTIMATE_SIZE = 90

/** 虚拟列表 gap（px），等同于 CSS --space-2 */
export const VIRTUAL_LIST_GAP = 8

/** 虚拟列表上下内边距（px），等同于 CSS --space-3 */
export const VIRTUAL_LIST_PADDING = 12

/** 虚拟列表前后额外渲染的条目数，防止快速滚动时白屏 */
export const VIRTUAL_LIST_OVERSCAN = 5
