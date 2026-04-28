#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use chrono::{Local, TimeZone};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tempfile::TempDir;

use enhanced_clipboard_lib::db::{Database, SettingsStore};
use enhanced_clipboard_lib::i18n::I18n;
use enhanced_clipboard_lib::models::{ClipboardEntry, ClipboardPreview};
use enhanced_clipboard_lib::services::persisted_state::PersistedApp;
use enhanced_clipboard_lib::services::search_preview::build_canonical_search_text;
use enhanced_clipboard_lib::services::settings::SettingsApp;
use enhanced_clipboard_lib::services::view_events::EventEmitter;

const TEST_DB_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub struct TestContext {
    pub _tempdir: TempDir,
    pub data_dir: PathBuf,
    pub db: Database,
    pub settings: SettingsStore,
}

impl TestContext {
    pub fn new() -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let data_dir = tempdir.path().join("data");
        std::fs::create_dir_all(data_dir.join("images")).expect("images dir");
        std::fs::create_dir_all(data_dir.join("thumbnails")).expect("thumbnails dir");

        let db = Database::new(
            data_dir.join("clipboard.db").to_string_lossy().as_ref(),
            TEST_DB_KEY,
            false,
        )
        .expect("clipboard db");
        let settings = SettingsStore::new(data_dir.join("settings.db").to_string_lossy().as_ref())
            .expect("settings db");

        Self {
            _tempdir: tempdir,
            data_dir,
            db,
            settings,
        }
    }
}

pub fn test_i18n() -> Arc<RwLock<I18n>> {
    Arc::new(RwLock::new(enhanced_clipboard_lib::i18n::load("en-US")))
}

#[derive(Default)]
pub struct TestApp {
    events: Mutex<Vec<(String, serde_json::Value)>>,
    autostart_calls: Mutex<Vec<bool>>,
    hotkey_calls: Mutex<Vec<String>>,
    always_on_top_calls: Mutex<Vec<bool>>,
    window_position_calls: Mutex<Vec<(i32, i32)>>,
    autostart_error: Mutex<Option<String>>,
    hotkey_error: Mutex<Option<String>>,
    always_on_top_error: Mutex<Option<String>>,
    window_position_error: Mutex<Option<String>>,
}

impl TestApp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fail_always_on_top(&self, message: impl Into<String>) {
        *self
            .always_on_top_error
            .lock()
            .expect("always_on_top_error") = Some(message.into());
    }

    pub fn fail_autostart(&self, message: impl Into<String>) {
        *self.autostart_error.lock().expect("autostart_error") = Some(message.into());
    }

    pub fn fail_hotkey(&self, message: impl Into<String>) {
        *self.hotkey_error.lock().expect("hotkey_error") = Some(message.into());
    }

    pub fn fail_window_position(&self, message: impl Into<String>) {
        *self
            .window_position_error
            .lock()
            .expect("window_position_error") = Some(message.into());
    }

    pub fn captured_event<T>(&self, event: &str) -> Vec<T>
    where
        T: DeserializeOwned,
    {
        self.events
            .lock()
            .expect("events")
            .iter()
            .filter(|(name, _)| name == event)
            .map(|(_, payload)| {
                serde_json::from_value::<T>(payload.clone()).expect("deserialize event")
            })
            .collect()
    }

    pub fn autostart_calls(&self) -> Vec<bool> {
        self.autostart_calls
            .lock()
            .expect("autostart_calls")
            .clone()
    }

    pub fn hotkey_calls(&self) -> Vec<String> {
        self.hotkey_calls.lock().expect("hotkey_calls").clone()
    }

    pub fn always_on_top_calls(&self) -> Vec<bool> {
        self.always_on_top_calls
            .lock()
            .expect("always_on_top_calls")
            .clone()
    }

    pub fn window_position_calls(&self) -> Vec<(i32, i32)> {
        self.window_position_calls
            .lock()
            .expect("window_position_calls")
            .clone()
    }
}

impl EventEmitter for TestApp {
    fn emit_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String> {
        self.events.lock().expect("events").push((
            event.to_string(),
            serde_json::to_value(payload).expect("serialize event"),
        ));
        Ok(())
    }
}

impl SettingsApp for TestApp {
    fn apply_autostart(&self, enabled: bool) -> Result<(), String> {
        self.autostart_calls
            .lock()
            .expect("autostart_calls")
            .push(enabled);
        if let Some(message) = self
            .autostart_error
            .lock()
            .expect("autostart_error")
            .clone()
        {
            Err(message)
        } else {
            Ok(())
        }
    }

    fn register_hotkey(&self, hotkey: &str) -> Result<(), String> {
        self.hotkey_calls
            .lock()
            .expect("hotkey_calls")
            .push(hotkey.to_string());
        if let Some(message) = self.hotkey_error.lock().expect("hotkey_error").clone() {
            Err(message)
        } else {
            Ok(())
        }
    }
}

impl PersistedApp for TestApp {
    fn set_always_on_top(&self, enabled: bool) -> Result<(), String> {
        self.always_on_top_calls
            .lock()
            .expect("always_on_top_calls")
            .push(enabled);
        if let Some(message) = self
            .always_on_top_error
            .lock()
            .expect("always_on_top_error")
            .clone()
        {
            Err(message)
        } else {
            Ok(())
        }
    }

    fn restore_window_position(&self, x: i32, y: i32) -> Result<(), String> {
        self.window_position_calls
            .lock()
            .expect("window_position_calls")
            .push((x, y));
        if let Some(message) = self
            .window_position_error
            .lock()
            .expect("window_position_error")
            .clone()
        {
            Err(message)
        } else {
            Ok(())
        }
    }
}

pub fn text_entry(id: &str, created_at: i64, content: &str) -> ClipboardEntry {
    ClipboardEntry {
        id: id.to_string(),
        content_type: "text".to_string(),
        content: content.to_string(),
        canonical_search_text: build_canonical_search_text(content),
        tags: Vec::new(),
        created_at,
        is_pinned: false,
        source_app: "Code".to_string(),
        image_path: None,
        thumbnail_path: None,
    }
}

pub fn image_entry(id: &str, created_at: i64) -> ClipboardEntry {
    ClipboardEntry {
        id: id.to_string(),
        content_type: "image".to_string(),
        content: String::new(),
        canonical_search_text: String::new(),
        tags: Vec::new(),
        created_at,
        is_pinned: false,
        source_app: "Photos".to_string(),
        image_path: Some(format!("images/{id}.png")),
        thumbnail_path: Some(format!("thumbnails/{id}.png")),
    }
}

pub fn pinned(mut entry: ClipboardEntry) -> ClipboardEntry {
    entry.is_pinned = true;
    entry
}

pub fn insert_entry(ctx: &TestContext, entry: &ClipboardEntry) {
    ctx.db.insert_entry(entry).expect("insert entry");
}

pub fn insert_entry_with_tags(ctx: &TestContext, entry: &ClipboardEntry, tags: &[&str]) {
    let tag_values = tags.iter().map(|tag| tag.to_string()).collect::<Vec<_>>();
    ctx.db
        .insert_entry_with_attrs(entry, &[("tag", tag_values.as_slice())])
        .expect("insert entry with tags");
}

pub fn touch_file(ctx: &TestContext, relative_path: &str) {
    let full_path = ctx.data_dir.join(relative_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).expect("create asset parent");
    }
    std::fs::write(full_path, b"asset").expect("write asset");
}

pub fn local_date(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .expect("local datetime")
        .format("%Y-%m-%d")
        .to_string()
}

pub fn local_month(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .expect("local datetime")
        .format("%Y-%m")
        .to_string()
}

pub fn text_preview_text(preview: &ClipboardPreview) -> &str {
    match preview {
        ClipboardPreview::Text { text, .. } => text,
        ClipboardPreview::Image { .. } => panic!("expected text preview"),
    }
}
