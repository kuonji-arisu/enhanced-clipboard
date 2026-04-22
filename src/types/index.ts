export interface ClipboardEntry {
  id: string
  content_type: 'text' | 'image'
  /** 文本条目内容；图片条目为空字符串。 */
  content: string
  /** 语义标签；无标签时为空数组。 */
  tags: string[]
  /** Unix epoch 秒 */
  created_at: number
  is_pinned: boolean
  source_app: string
  /** 原图绝对路径，如 `.../images/uuid.png`；文本条目为 null/undefined。 */
  image_path?: string | null
  /** 缩略图绝对路径，如 `.../thumbnails/uuid.jpg`；生成完成前为 null/undefined。 */
  thumbnail_path?: string | null
}

export type ClipboardEntryType = ClipboardEntry['content_type']

export interface TextRange {
  start: number
  end: number
}

export const CLIPBOARD_PREVIEW_KIND = {
  PREFIX: 'prefix',
  SEARCH_SNIPPET: 'search_snippet',
  IMAGE_PENDING: 'image_pending',
  IMAGE_READY: 'image_ready',
} as const

export type ClipboardPreviewKind =
  typeof CLIPBOARD_PREVIEW_KIND[keyof typeof CLIPBOARD_PREVIEW_KIND]

export const CLIPBOARD_QUERY_STALE_REASON = {
  ENTRY_CREATED: 'entry_created',
  ENTRY_UPDATED: 'entry_updated',
  ENTRIES_REMOVED: 'entries_removed',
  ENTRY_REMOVED: 'entry_removed',
  CLEAR_ALL: 'clear_all',
  PIN_CHANGED: 'pin_changed',
  UNPIN_RETENTION: 'unpin_retention',
  TTL_EXPIRED: 'ttl_expired',
  BEFORE_INSERT: 'before_insert',
  SETTINGS_OR_STARTUP: 'settings_or_startup',
} as const

export type ClipboardQueryStaleReason =
  typeof CLIPBOARD_QUERY_STALE_REASON[keyof typeof CLIPBOARD_QUERY_STALE_REASON]

export interface ClipboardListItem {
  id: string
  content_type: ClipboardEntryType
  /** 语义标签；无标签时为空数组。 */
  tags: string[]
  /** Unix epoch 秒 */
  created_at: number
  is_pinned: boolean
  source_app: string
  /** 列表专用预览文本；不代表 raw ClipboardEntry.content。 */
  preview_text: string
  preview_kind: ClipboardPreviewKind
  /** 基于 preview_text 的字符范围。 */
  match_ranges: TextRange[]
  /** 原图绝对路径；列表展示仍只使用 thumbnail_path。 */
  image_path?: string | null
  /** 缩略图绝对路径；生成完成前为 null/undefined。 */
  thumbnail_path?: string | null
}

export interface ClipboardQueryCursor {
  createdAt: number
  id: string
}

export interface ClipboardEntriesQuery {
  text?: string
  entryType?: ClipboardEntryType
  tag?: string
  date?: string
  cursor?: ClipboardQueryCursor
  limit?: number
}

export interface AppInfo {
  locale: string
  version: string
  os: string
  default_hotkey: string
  default_max_history: number
  min_history_limit: number
  max_history_limit: number
  page_size: number
  max_pinned_entries: number
  expiry_presets: number[]
  log_level_options: Array<'silent' | 'error' | 'warning' | 'info' | 'debug'>
}

export type ThemeMode = 'light' | 'dark' | 'system'
export type SystemTheme = 'light' | 'dark'
export type EffectiveTheme = SystemTheme

export interface AppSettings {
  hotkey: string
  autostart: boolean
  max_history: number
  theme_mode: ThemeMode
  /** 自动过期时长（秒），0 表示永不过期 */
  expiry_seconds: number
  /** 是否捕获图片剪贴板内容 */
  capture_images: boolean
  /** 后端文件日志等级 */
  log_level: 'silent' | 'error' | 'warning' | 'info' | 'debug'
}

export interface AppSettingsPatch {
  hotkey?: string
  autostart?: boolean
  max_history?: number
  theme_mode?: ThemeMode
  expiry_seconds?: number
  capture_images?: boolean
  log_level?: 'silent' | 'error' | 'warning' | 'info' | 'debug'
}

export interface EffectResult {
  ok: boolean
  error?: string
}

export interface SaveSettingsResult {
  settings: AppSettings
  effects: {
    autostart?: EffectResult
    hotkey?: EffectResult
    retention?: EffectResult
    log_level?: EffectResult
  }
}

export interface PersistedState {
  /** 上次保存的窗口 X 坐标；未保存时为 null。 */
  window_x: number | null
  /** 上次保存的窗口 Y 坐标；未保存时为 null。 */
  window_y: number | null
  /** 是否保持窗口置顶。 */
  always_on_top: boolean
}

export interface PersistedStatePatch {
  window_x?: number | null
  window_y?: number | null
  always_on_top?: boolean
}

export interface SavePersistedResult {
  persisted: PersistedState
  effects?: {
    always_on_top?: EffectResult
  }
}

export interface RuntimeStatus {
  clipboard_capture_available: boolean
  system_theme: SystemTheme
}

export interface RuntimeStatusPatch {
  clipboard_capture_available?: boolean
  system_theme?: SystemTheme
}
