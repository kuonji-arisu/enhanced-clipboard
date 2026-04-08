use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::constants::{
    DEFAULT_CAPTURE_IMAGES, DEFAULT_EXPIRY_SECONDS, DEFAULT_HOTKEY, DEFAULT_MAX_HISTORY,
    DEFAULT_LOG_LEVEL, DEFAULT_THEME, MAX_HISTORY_ENTRIES, MIN_HISTORY_ENTRIES,
};
use crate::models::AppSettings;

/// settings 表中的键名常量
const KEY_HOTKEY: &str = "hotkey";
const KEY_AUTOSTART: &str = "autostart";
const KEY_MAX_HISTORY: &str = "max_history";
const KEY_THEME: &str = "theme";
const KEY_LANGUAGE: &str = "language";
const KEY_EXPIRY: &str = "expiry_seconds";
const KEY_CAPTURE_IMAGES: &str = "capture_images";
const KEY_LOG_LEVEL: &str = "log_level";

/// 管理 `settings` 键值表，使用独立的 settings.db 文件。
pub struct SettingsStore {
    conn: Mutex<Connection>,
}

impl SettingsStore {
    fn sanitize_hotkey(hotkey: &str) -> String {
        hotkey.trim().to_string()
    }

    fn sanitize_theme(theme: &str) -> String {
        match theme.trim() {
            "light" => "light".to_string(),
            "dark" => "dark".to_string(),
            _ => DEFAULT_THEME.to_string(),
        }
    }

    fn sanitize_language(language: &str) -> String {
        match language.trim() {
            "" => String::new(),
            "zh" => "zh".to_string(),
            "en" => "en".to_string(),
            _ => String::new(),
        }
    }

    fn sanitize_expiry_seconds(expiry_seconds: i64) -> i64 {
        expiry_seconds.max(0)
    }

    fn sanitize_log_level(log_level: &str) -> String {
        match log_level.trim().to_ascii_lowercase().as_str() {
            "silent" => "silent".to_string(),
            "error" => "error".to_string(),
            "warning" => "warning".to_string(),
            "info" => "info".to_string(),
            "debug" => "debug".to_string(),
            _ => DEFAULT_LOG_LEVEL.to_string(),
        }
    }

    pub fn sanitize_app_settings(s: &AppSettings) -> AppSettings {
        AppSettings {
            hotkey: Self::sanitize_hotkey(&s.hotkey),
            theme: Self::sanitize_theme(&s.theme),
            language: Self::sanitize_language(&s.language),
            expiry_seconds: Self::sanitize_expiry_seconds(s.expiry_seconds),
            capture_images: s.capture_images,
            log_level: Self::sanitize_log_level(&s.log_level),
            max_history: s.max_history.clamp(MIN_HISTORY_ENTRIES, MAX_HISTORY_ENTRIES),
            ..s.clone()
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

    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|e| e.to_string())?;
        Ok(stmt.query_row(params![key], |row| row.get(0)).ok())
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 一次查询加载全部设置项，减少锁竞争和 SQL 开销。
    pub fn load_app_settings(&self) -> Result<AppSettings, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings")
            .map_err(|e| e.to_string())?;
        let map: HashMap<String, String> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Self::sanitize_app_settings(&AppSettings {
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
            language: map.get(KEY_LANGUAGE).cloned().unwrap_or_default(),
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
                .map(|v| Self::sanitize_log_level(&v))
                .unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_string()),
        }))
    }

    /// 在单个事务中保存全部设置项，保证原子性。
    pub fn save_app_settings(&self, s: &AppSettings) -> Result<(), String> {
        let sanitized = Self::sanitize_app_settings(s);
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let sql = "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)";
        tx.execute(sql, params![KEY_HOTKEY, sanitized.hotkey])
            .map_err(|e| e.to_string())?;
        tx.execute(sql, params![KEY_AUTOSTART, sanitized.autostart.to_string()])
            .map_err(|e| e.to_string())?;
        tx.execute(
            sql,
            params![KEY_MAX_HISTORY, sanitized.max_history.to_string()],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(sql, params![KEY_THEME, sanitized.theme])
            .map_err(|e| e.to_string())?;
        tx.execute(sql, params![KEY_LANGUAGE, sanitized.language])
            .map_err(|e| e.to_string())?;
        tx.execute(
            sql,
            params![KEY_EXPIRY, sanitized.expiry_seconds.to_string()],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            sql,
            params![KEY_CAPTURE_IMAGES, sanitized.capture_images.to_string()],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(sql, params![KEY_LOG_LEVEL, sanitized.log_level])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
}
