use serde::{Deserialize, Serialize};

use crate::constants::{
    DEFAULT_CAPTURE_IMAGES, DEFAULT_EXPIRY_SECONDS, DEFAULT_HOTKEY, DEFAULT_MAX_HISTORY,
    DEFAULT_LOG_LEVEL, DEFAULT_THEME,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub id: String,
    pub content_type: String,
    /// 文本条目内容；图片条目为空字符串。
    pub content: String,
    /// Unix epoch 秒
    pub created_at: i64,
    pub is_pinned: bool,
    pub source_app: String,
    /// 原图相对路径，如 `images/uuid.png`（由服务层解析为绝对路径后发送给前端）
    pub image_path: Option<String>,
    /// 缩略图相对路径，如 `thumbnails/uuid.jpg`（生成后填充）
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub hotkey: String,
    pub autostart: bool,
    pub max_history: u32,
    pub theme: String,
    /// 空字符串表示跟随系统语言；"zh" 或 "en" 为显式指定
    pub language: String,
    /// 自动过期时长（秒），0 表示永不过期
    pub expiry_seconds: i64,
    /// 是否捕获图片剪贴板内容
    pub capture_images: bool,
    /// 后端文件日志等级：silent / error / info / debug
    pub log_level: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_HOTKEY.to_string(),
            autostart: false,
            max_history: DEFAULT_MAX_HISTORY,
            theme: DEFAULT_THEME.to_string(),
            language: String::new(),
            expiry_seconds: DEFAULT_EXPIRY_SECONDS,
            capture_images: DEFAULT_CAPTURE_IMAGES,
            log_level: DEFAULT_LOG_LEVEL.to_string(),
        }
    }
}

/// 包装数据根目录路径的新类型，注册为 Tauri 应用状态。
/// 图片和缩略图均存储在此目录的子目录中（images/ 和 thumbnails/）。
pub struct DataDir(pub std::path::PathBuf);

/// `entry_updated` 事件的有类型载荷，确保 Rust 与前端监听器的字段名一致。
#[derive(Debug, Clone, Serialize)]
pub struct ImageUpdatePayload {
    pub id: String,
    /// 原图绝对路径（前端不显示，仅内部用于 copy_entry 的 asset 协议）
    pub image_path: String,
    /// 缩略图绝对路径；前端的唯一展示源
    pub thumbnail_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeStatus {
    pub clipboard_capture_available: bool,
}

pub struct RuntimeStatusState(pub std::sync::Mutex<RuntimeStatus>);
