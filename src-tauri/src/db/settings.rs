use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::constants::{
    DEFAULT_CAPTURE_IMAGES, DEFAULT_EXPIRY_SECONDS, DEFAULT_HOTKEY, DEFAULT_LOG_LEVEL,
    DEFAULT_MAX_HISTORY, DEFAULT_THEME, MAX_HISTORY_ENTRIES, MIN_HISTORY_ENTRIES,
};
use crate::models::{AppSettings, PersistedField, PersistedState, SettingsField};

/// settings 表中的键名常量
const KEY_HOTKEY: &str = "hotkey";
const KEY_AUTOSTART: &str = "autostart";
const KEY_MAX_HISTORY: &str = "max_history";
const KEY_THEME: &str = "theme";
const KEY_LANGUAGE: &str = "language";
const KEY_EXPIRY: &str = "expiry_seconds";
const KEY_CAPTURE_IMAGES: &str = "capture_images";
const KEY_LOG_LEVEL: &str = "log_level";
const KEY_WINDOW_X: &str = "window_x";
const KEY_WINDOW_Y: &str = "window_y";
const KEY_ALWAYS_ON_TOP: &str = "always_on_top";

/// 管理 `settings` 键值表，使用独立的 settings.db 文件。
pub struct SettingsStore {
    conn: Mutex<Connection>,
}

impl SettingsStore {
    fn load_settings_map(conn: &Connection) -> Result<HashMap<String, String>, String> {
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings")
            .map_err(|e| e.to_string())?;
        let map = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect::<HashMap<String, String>>();
        Ok(map)
    }

    fn set_key(tx: &rusqlite::Transaction<'_>, key: &str, value: String) -> Result<(), String> {
        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn set_optional_key(
        tx: &rusqlite::Transaction<'_>,
        key: &str,
        value: Option<i32>,
    ) -> Result<(), String> {
        match value {
            Some(value) => Self::set_key(tx, key, value.to_string()),
            None => tx
                .execute("DELETE FROM settings WHERE key = ?1", params![key])
                .map(|_| ())
                .map_err(|e| e.to_string()),
        }
    }

    fn normalize_theme(theme: &str) -> String {
        match theme.trim() {
            "light" => "light".to_string(),
            "dark" => "dark".to_string(),
            _ => DEFAULT_THEME.to_string(),
        }
    }

    fn normalize_expiry_seconds(expiry_seconds: i64) -> i64 {
        expiry_seconds.max(0)
    }

    fn normalize_log_level(log_level: &str) -> String {
        match log_level.trim().to_ascii_lowercase().as_str() {
            "silent" => "silent".to_string(),
            "error" => "error".to_string(),
            "warning" => "warning".to_string(),
            "info" => "info".to_string(),
            "debug" => "debug".to_string(),
            _ => DEFAULT_LOG_LEVEL.to_string(),
        }
    }

    pub fn normalize_runtime_app_settings(settings: &AppSettings) -> AppSettings {
        AppSettings {
            hotkey: settings.hotkey.trim().to_string(),
            autostart: settings.autostart,
            max_history: settings
                .max_history
                .clamp(MIN_HISTORY_ENTRIES, MAX_HISTORY_ENTRIES),
            theme: Self::normalize_theme(&settings.theme),
            expiry_seconds: Self::normalize_expiry_seconds(settings.expiry_seconds),
            capture_images: settings.capture_images,
            log_level: Self::normalize_log_level(&settings.log_level),
        }
    }

    pub fn new(db_path: &str) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open settings database: {}", e))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=3000;")
            .map_err(|e| format!("PRAGMA init failed: {}", e))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                 key   TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )
        .map_err(|e| format!("Failed to init settings table: {}", e))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 一次查询加载全部设置项，减少锁竞争和 SQL 开销。
    pub fn load_app_settings(&self) -> Result<AppSettings, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let map = Self::load_settings_map(&conn)?;

        Ok(AppSettings {
            hotkey: map
                .get(KEY_HOTKEY)
                .cloned()
                .unwrap_or_else(|| DEFAULT_HOTKEY.to_string()),
            autostart: map.get(KEY_AUTOSTART).map(|v| v == "true").unwrap_or(false),
            max_history: map
                .get(KEY_MAX_HISTORY)
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_MAX_HISTORY),
            theme: map
                .get(KEY_THEME)
                .cloned()
                .unwrap_or_else(|| DEFAULT_THEME.to_string()),
            expiry_seconds: map
                .get(KEY_EXPIRY)
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_EXPIRY_SECONDS),
            capture_images: map
                .get(KEY_CAPTURE_IMAGES)
                .map(|v| v == "true")
                .unwrap_or(DEFAULT_CAPTURE_IMAGES),
            log_level: map
                .get(KEY_LOG_LEVEL)
                .cloned()
                .unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_string()),
        })
    }

    pub fn load_runtime_app_settings(&self) -> Result<AppSettings, String> {
        self.load_app_settings()
            .map(|settings| Self::normalize_runtime_app_settings(&settings))
    }

    pub fn load_persisted_state(&self) -> Result<PersistedState, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let map = Self::load_settings_map(&conn)?;

        Ok(PersistedState {
            window_x: map.get(KEY_WINDOW_X).and_then(|v| v.parse().ok()),
            window_y: map.get(KEY_WINDOW_Y).and_then(|v| v.parse().ok()),
            always_on_top: map
                .get(KEY_ALWAYS_ON_TOP)
                .map(|v| v == "true")
                .unwrap_or(false),
        })
    }

    fn write_app_settings_field(
        tx: &rusqlite::Transaction<'_>,
        settings: &AppSettings,
        field: SettingsField,
    ) -> Result<(), String> {
        match field {
            SettingsField::Hotkey => Self::set_key(tx, KEY_HOTKEY, settings.hotkey.clone()),
            SettingsField::Autostart => {
                Self::set_key(tx, KEY_AUTOSTART, settings.autostart.to_string())
            }
            SettingsField::MaxHistory => {
                Self::set_key(tx, KEY_MAX_HISTORY, settings.max_history.to_string())
            }
            SettingsField::Theme => Self::set_key(tx, KEY_THEME, settings.theme.clone()),
            SettingsField::ExpirySeconds => {
                Self::set_key(tx, KEY_EXPIRY, settings.expiry_seconds.to_string())
            }
            SettingsField::CaptureImages => {
                Self::set_key(tx, KEY_CAPTURE_IMAGES, settings.capture_images.to_string())
            }
            SettingsField::LogLevel => Self::set_key(tx, KEY_LOG_LEVEL, settings.log_level.clone()),
        }
    }

    fn write_persisted_field(
        tx: &rusqlite::Transaction<'_>,
        state: &PersistedState,
        field: PersistedField,
    ) -> Result<(), String> {
        match field {
            PersistedField::WindowX => Self::set_optional_key(tx, KEY_WINDOW_X, state.window_x),
            PersistedField::WindowY => Self::set_optional_key(tx, KEY_WINDOW_Y, state.window_y),
            PersistedField::AlwaysOnTop => {
                Self::set_key(tx, KEY_ALWAYS_ON_TOP, state.always_on_top.to_string())
            }
        }
    }

    pub fn save_app_settings_fields(
        &self,
        settings: &AppSettings,
        fields: &[SettingsField],
    ) -> Result<(), String> {
        if fields.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM settings WHERE key = ?1", params![KEY_LANGUAGE])
            .map_err(|e| e.to_string())?;
        for field in fields {
            Self::write_app_settings_field(&tx, settings, *field)?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn save_persisted_state_fields(
        &self,
        state: &PersistedState,
        fields: &[PersistedField],
    ) -> Result<(), String> {
        if fields.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for field in fields {
            Self::write_persisted_field(&tx, state, *field)?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
}
