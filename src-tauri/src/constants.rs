/// 跨模块共享的常量。
/// 此文件是 Rust 侧唯一的常量声明来源，各模块通过 `use crate::constants::*` 引入。

// ── 设置默认值 ────────────────────────────────────────────────────────────────

/// 默认全局热键
pub const DEFAULT_HOTKEY: &str = "CmdOrCtrl+Shift+V";

/// 默认最大历史记录条数
pub const DEFAULT_MAX_HISTORY: u32 = 500;

/// 默认主题模式
pub const DEFAULT_THEME_MODE: &str = "light";

/// 默认自动过期秒数（0 = 永不过期）
pub const DEFAULT_EXPIRY_SECONDS: i64 = 0;

/// 默认启用图片捕获
pub const DEFAULT_CAPTURE_IMAGES: bool = true;

/// 默认日志等级
pub const DEFAULT_LOG_LEVEL: &str = "error";

/// 日志文件名（位于应用数据根目录）
pub const LOG_FILE_NAME: &str = "app.log";

/// 单个日志文件的最大大小，超出后直接删除并重建。
pub const MAX_LOG_FILE_BYTES: u64 = 5 * 1024 * 1024;

// ── 业务限制 ──────────────────────────────────────────────────────────────────

/// 同时允许置顶的最大条目数
pub const MAX_PINNED_ENTRIES: u32 = 3;

/// 历史记录条数下限（设置页面滑块最小值）
pub const MIN_HISTORY_ENTRIES: u32 = 10;

/// 历史记录条数上限（设置页面滑块最大值，与前端 MAX_HISTORY 保持一致）
pub const MAX_HISTORY_ENTRIES: u32 = 10000;

/// 列表分页大小
pub const PAGE_SIZE: u32 = 50;

/// 列表展示时文本条目的最大字符数
pub const DISPLAY_CONTENT_CHARS: usize = 200;

/// 自动过期预设选项（秒），0 表示永不过期。
pub const EXPIRY_PRESETS: &[i64] = &[0, 10 * 60, 30 * 60, 60 * 60, 24 * 60 * 60, 7 * 24 * 60 * 60];

/// 后端文件日志等级选项。
pub const LOG_LEVEL_OPTIONS: &[&str] = &["silent", "error", "warning", "info", "debug"];

// ── 窗口 / 托盘标识符 ─────────────────────────────────────────────────────────

/// 主窗口标识符（与 tauri.conf.json 中 label 保持一致）
pub const MAIN_WINDOW_LABEL: &str = "main";

/// 凭据库 service 名称（Windows Credential Manager / macOS Keychain 等）
pub const KEYRING_SERVICE: &str = "tech.kuon.enhanced-clipboard";

/// 开机自启传入的 CLI 参数
pub const AUTOSTART_ARG: &str = "--autostart";

// ── 事件名 ────────────────────────────────────────────────────────────────────

/// 新条目添加事件
pub const EVENT_ENTRY_ADDED: &str = "entry_added";

/// 条目更新事件（完整条目最终状态更新）
pub const EVENT_ENTRY_UPDATED: &str = "entry_updated";

/// 条目批量移除事件（prune）
pub const EVENT_ENTRIES_REMOVED: &str = "entries_removed";

/// 运行时状态 patch 事件；前端初始化后仅接收增量更新。
pub const EVENT_RUNTIME_STATUS_UPDATED: &str = "runtime_status_updated";
