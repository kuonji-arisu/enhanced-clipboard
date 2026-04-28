import type {
  AppInfo,
  AppSettings,
  ClipboardListItem,
  RuntimeStatus,
} from '../../../types'

export function createAppInfo(overrides: Partial<AppInfo> = {}): AppInfo {
  return {
    locale: 'en-US',
    version: '0.2.6',
    os: 'windows',
    default_hotkey: 'CmdOrCtrl+Shift+V',
    default_max_history: 500,
    min_history_limit: 10,
    max_history_limit: 10000,
    page_size: 50,
    max_pinned_entries: 3,
    expiry_presets: [0, 600, 1800, 3600],
    log_level_options: ['silent', 'error', 'warning', 'info', 'debug'],
    ...overrides,
  }
}

export function createAppSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    hotkey: 'CmdOrCtrl+Shift+V',
    autostart: false,
    max_history: 500,
    theme_mode: 'light',
    expiry_seconds: 0,
    capture_images: true,
    log_level: 'error',
    ...overrides,
  }
}

export function createRuntimeStatus(overrides: Partial<RuntimeStatus> = {}): RuntimeStatus {
  return {
    clipboard_capture_available: true,
    system_theme: 'light',
    ...overrides,
  }
}

export function createTextListItem(
  overrides: Partial<ClipboardListItem> = {},
): ClipboardListItem {
  return {
    id: 'entry-1',
    content_type: 'text',
    tags: [],
    created_at: 1_700_000_000,
    is_pinned: false,
    source_app: 'Code',
    preview: {
      kind: 'text',
      mode: 'prefix',
      text: 'Alpha',
      highlight_ranges: [],
    },
    image_path: null,
    thumbnail_path: null,
    ...overrides,
  }
}

export function createImageListItem(
  overrides: Partial<ClipboardListItem> = {},
): ClipboardListItem {
  return {
    id: 'image-1',
    content_type: 'image',
    tags: [],
    created_at: 1_700_000_000,
    is_pinned: false,
    source_app: 'Photos',
    preview: {
      kind: 'image',
      mode: 'ready',
    },
    image_path: 'C:/images/image.png',
    thumbnail_path: 'C:/thumbnails/image.png',
    ...overrides,
  }
}
