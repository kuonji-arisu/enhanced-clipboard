export interface ClipboardEntry {
  id: string
  content_type: 'text' | 'image'
  /** 文本条目内容；图片条目为空字符串。 */
  content: string
  /** Unix epoch 秒 */
  created_at: number
  is_pinned: boolean
  source_app: string
  /** 原图绝对路径，如 `.../images/uuid.png`；文本条目为 null/undefined。 */
  image_path?: string | null
  /** 缩略图绝对路径，如 `.../thumbnails/uuid.jpg`；生成完成前为 null/undefined。 */
  thumbnail_path?: string | null
}

export interface AppRuntimeInfo {
  locale: string
  version: string
  os: string
}

export interface AppConstants {
  default_hotkey: string
  default_max_history: number
  min_history_limit: number
  max_history_limit: number
  page_size: number
  max_pinned_entries: number
  expiry_presets: number[]
  log_level_options: Array<'silent' | 'error' | 'warning' | 'info' | 'debug'>
}

export interface AppInfo {
  runtime: AppRuntimeInfo
  constants: AppConstants
}

export interface AppSettings {
  hotkey: string
  autostart: boolean
  max_history: number
  theme: 'light' | 'dark'
  language: string
  /** 自动过期时长（秒），0 表示永不过期 */
  expiry_seconds: number
  /** 是否捕获图片剪贴板内容 */
  capture_images: boolean
  /** 后端文件日志等级 */
  log_level: 'silent' | 'error' | 'warning' | 'info' | 'debug'
}

export interface RuntimeStatus {
  clipboard_capture_available: boolean
}
