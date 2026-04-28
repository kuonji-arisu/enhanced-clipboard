use serde::{Deserialize, Serialize};

use crate::constants::{
    DEFAULT_CAPTURE_IMAGES, DEFAULT_EXPIRY_SECONDS, DEFAULT_HOTKEY, DEFAULT_LOG_LEVEL,
    DEFAULT_MAX_HISTORY, DEFAULT_THEME_MODE, PAGE_SIZE,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub id: String,
    pub content_type: String,
    /// 文本条目内容；图片条目为空字符串。
    pub content: String,
    #[serde(default, skip)]
    pub canonical_search_text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Unix epoch 秒
    pub created_at: i64,
    pub is_pinned: bool,
    pub source_app: String,
    /// 原图相对路径，如 `images/uuid.png`（由服务层解析为绝对路径后发送给前端）
    pub image_path: Option<String>,
    /// 图片列表展示入口，如 `thumbnails/uuid.png` 或 `thumbnails/uuid.jpg`。不复用原图路径。
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardTextPreviewMode {
    Prefix,
    SearchSnippet,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardImagePreviewMode {
    Pending,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipboardPreview {
    Text {
        mode: ClipboardTextPreviewMode,
        text: String,
        #[serde(default)]
        highlight_ranges: Vec<TextRange>,
    },
    Image {
        mode: ClipboardImagePreviewMode,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardQueryStaleReason {
    EntryCreated,
    EntryUpdated,
    EntriesRemoved,
    EntryRemoved,
    ClearAll,
    PinChanged,
    UnpinRetention,
    TtlExpired,
    BeforeInsert,
    SettingsOrStartup,
}

impl ClipboardQueryStaleReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EntryCreated => "entry_created",
            Self::EntryUpdated => "entry_updated",
            Self::EntriesRemoved => "entries_removed",
            Self::EntryRemoved => "entry_removed",
            Self::ClearAll => "clear_all",
            Self::PinChanged => "pin_changed",
            Self::UnpinRetention => "unpin_retention",
            Self::TtlExpired => "ttl_expired",
            Self::BeforeInsert => "before_insert",
            Self::SettingsOrStartup => "settings_or_startup",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardListItem {
    pub id: String,
    pub content_type: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Unix epoch 秒
    pub created_at: i64,
    pub is_pinned: bool,
    pub source_app: String,
    /// UI 列表专用预览对象；不代表原始 clipboard content。
    pub preview: ClipboardPreview,
    /// 原图绝对路径；仅供少量 UI 元数据场景使用，列表展示仍以 thumbnail_path 为准。
    pub image_path: Option<String>,
    /// 图片条目的唯一列表展示源；不应与 image_path 指向同一文件。
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardQueryCursor {
    pub created_at: i64,
    pub id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClipboardEntryType {
    Text,
    Image,
}

impl ClipboardEntryType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct ClipboardEntriesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(rename = "entryType", skip_serializing_if = "Option::is_none")]
    pub entry_type: Option<ClipboardEntryType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ClipboardQueryCursor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl ClipboardEntriesQuery {
    pub fn text(&self) -> Option<&str> {
        self.text
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref().filter(|value| !value.trim().is_empty())
    }

    pub fn entry_type(&self) -> Option<ClipboardEntryType> {
        self.entry_type
    }

    pub fn date(&self) -> Option<&str> {
        self.date
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn normalized_limit(&self) -> u32 {
        self.limit.unwrap_or(PAGE_SIZE).clamp(1, PAGE_SIZE)
    }

    pub fn is_first_page(&self) -> bool {
        self.cursor.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    pub hotkey: String,
    pub autostart: bool,
    pub max_history: u32,
    pub theme_mode: String,
    /// 自动过期时长（秒），0 表示永不过期
    pub expiry_seconds: i64,
    /// 是否捕获图片剪贴板内容
    pub capture_images: bool,
    /// 后端文件日志等级：silent / error / info / debug
    pub log_level: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AppSettingsPatch {
    pub hotkey: Option<String>,
    pub autostart: Option<bool>,
    pub max_history: Option<u32>,
    pub theme_mode: Option<String>,
    /// 自动过期时长（秒），0 表示永不过期
    pub expiry_seconds: Option<i64>,
    /// 是否捕获图片剪贴板内容
    pub capture_images: Option<bool>,
    /// 后端文件日志等级：silent / error / info / debug
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceDomain {
    Settings,
    Persisted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStrategy {
    PersistOnly,
    PersistThenApply,
    ApplyThenPersist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsEffectKey {
    Autostart,
    Hotkey,
    Retention,
    CaptureImages,
    LogLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedEffectKey {
    AlwaysOnTop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMetadata<EffectKey>
where
    EffectKey: Copy,
{
    pub domain: PersistenceDomain,
    pub strategy: SaveStrategy,
    pub effect: Option<EffectKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    Hotkey,
    Autostart,
    MaxHistory,
    ThemeMode,
    ExpirySeconds,
    CaptureImages,
    LogLevel,
}

impl SettingsField {
    pub const ALL: [Self; 7] = [
        Self::Hotkey,
        Self::Autostart,
        Self::MaxHistory,
        Self::ThemeMode,
        Self::ExpirySeconds,
        Self::CaptureImages,
        Self::LogLevel,
    ];

    pub fn metadata(self) -> FieldMetadata<SettingsEffectKey> {
        match self {
            Self::Hotkey => FieldMetadata {
                domain: PersistenceDomain::Settings,
                strategy: SaveStrategy::PersistThenApply,
                effect: Some(SettingsEffectKey::Hotkey),
            },
            Self::Autostart => FieldMetadata {
                domain: PersistenceDomain::Settings,
                strategy: SaveStrategy::PersistThenApply,
                effect: Some(SettingsEffectKey::Autostart),
            },
            Self::MaxHistory => FieldMetadata {
                domain: PersistenceDomain::Settings,
                strategy: SaveStrategy::PersistThenApply,
                effect: Some(SettingsEffectKey::Retention),
            },
            Self::ThemeMode => FieldMetadata {
                domain: PersistenceDomain::Settings,
                strategy: SaveStrategy::PersistOnly,
                effect: None,
            },
            Self::ExpirySeconds => FieldMetadata {
                domain: PersistenceDomain::Settings,
                strategy: SaveStrategy::PersistThenApply,
                effect: Some(SettingsEffectKey::Retention),
            },
            Self::CaptureImages => FieldMetadata {
                domain: PersistenceDomain::Settings,
                strategy: SaveStrategy::PersistThenApply,
                effect: Some(SettingsEffectKey::CaptureImages),
            },
            Self::LogLevel => FieldMetadata {
                domain: PersistenceDomain::Settings,
                strategy: SaveStrategy::PersistThenApply,
                effect: Some(SettingsEffectKey::LogLevel),
            },
        }
    }

    pub fn changed(self, current: &AppSettings, next: &AppSettings) -> bool {
        match self {
            Self::Hotkey => current.hotkey != next.hotkey,
            Self::Autostart => current.autostart != next.autostart,
            Self::MaxHistory => current.max_history != next.max_history,
            Self::ThemeMode => current.theme_mode != next.theme_mode,
            Self::ExpirySeconds => current.expiry_seconds != next.expiry_seconds,
            Self::CaptureImages => current.capture_images != next.capture_images,
            Self::LogLevel => current.log_level != next.log_level,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PersistedStatePatch {
    /// 上次保存的窗口 X 坐标；None 表示不修改该字段
    pub window_x: Option<Option<i32>>,
    /// 上次保存的窗口 Y 坐标；None 表示不修改该字段
    pub window_y: Option<Option<i32>>,
    /// 是否保持窗口置顶；None 表示不修改该字段
    pub always_on_top: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PersistedState {
    /// 上次保存的窗口 X 坐标；未保存时为 None
    pub window_x: Option<i32>,
    /// 上次保存的窗口 Y 坐标；未保存时为 None
    pub window_y: Option<i32>,
    /// 是否保持窗口置顶
    pub always_on_top: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedField {
    WindowX,
    WindowY,
    AlwaysOnTop,
}

impl PersistedField {
    pub const ALL: [Self; 3] = [Self::WindowX, Self::WindowY, Self::AlwaysOnTop];

    pub fn metadata(self) -> FieldMetadata<PersistedEffectKey> {
        match self {
            Self::WindowX => FieldMetadata {
                domain: PersistenceDomain::Persisted,
                strategy: SaveStrategy::PersistOnly,
                effect: None,
            },
            Self::WindowY => FieldMetadata {
                domain: PersistenceDomain::Persisted,
                strategy: SaveStrategy::PersistOnly,
                effect: None,
            },
            Self::AlwaysOnTop => FieldMetadata {
                domain: PersistenceDomain::Persisted,
                strategy: SaveStrategy::ApplyThenPersist,
                effect: Some(PersistedEffectKey::AlwaysOnTop),
            },
        }
    }

    pub fn changed(self, current: &PersistedState, next: &PersistedState) -> bool {
        match self {
            Self::WindowX => current.window_x != next.window_x,
            Self::WindowY => current.window_y != next.window_y,
            Self::AlwaysOnTop => current.always_on_top != next.always_on_top,
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_HOTKEY.to_string(),
            autostart: false,
            max_history: DEFAULT_MAX_HISTORY,
            theme_mode: DEFAULT_THEME_MODE.to_string(),
            expiry_seconds: DEFAULT_EXPIRY_SECONDS,
            capture_images: DEFAULT_CAPTURE_IMAGES,
            log_level: DEFAULT_LOG_LEVEL.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub locale: String,
    pub version: String,
    pub os: String,
    pub default_hotkey: String,
    pub default_max_history: u32,
    pub min_history_limit: u32,
    pub max_history_limit: u32,
    pub page_size: u32,
    pub max_pinned_entries: u32,
    pub expiry_presets: Vec<i64>,
    pub log_level_options: Vec<String>,
}

pub struct AppInfoState(pub AppInfo);

/// 包装数据根目录路径的新类型，注册为 Tauri 应用状态。
/// 图片和缩略图均存储在此目录的子目录中（images/ 和 thumbnails/）。
pub struct DataDir(pub std::path::PathBuf);

/// 只读运行时快照；仅表达当前进程里的真实动态状态。
/// 后续新增系统主题、热键注册结果等动态字段时，统一加在这里，
/// 不要混入 settings / persisted。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub clipboard_capture_available: bool,
    pub system_theme: String,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self {
            clipboard_capture_available: true,
            system_theme: "light".to_string(),
        }
    }
}

/// 运行时增量更新载荷；统一用于后端 merge 和前端事件 patch。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RuntimeStatusPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard_capture_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_theme: Option<String>,
}

impl RuntimeStatusPatch {
    pub fn is_empty(&self) -> bool {
        self.clipboard_capture_available.is_none() && self.system_theme.is_none()
    }
}

pub struct RuntimeStatusState(pub std::sync::Mutex<RuntimeStatus>);

#[derive(Debug, Clone, Serialize, Default)]
pub struct EffectResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SaveSettingsEffects {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autostart: Option<EffectResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<EffectResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention: Option<EffectResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_images: Option<EffectResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<EffectResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveSettingsResult {
    pub settings: AppSettings,
    pub effects: SaveSettingsEffects,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SavePersistedEffects {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_on_top: Option<EffectResult>,
}

impl SavePersistedEffects {
    pub fn is_empty(&self) -> bool {
        self.always_on_top.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SavePersistedResult {
    pub persisted: PersistedState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effects: Option<SavePersistedEffects>,
}
