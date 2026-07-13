#![allow(dead_code)]

use std::sync::{Arc, Mutex, RwLock};

use serde::de::DeserializeOwned;
use serde::Serialize;
use tempfile::TempDir;

use enhanced_clipboard_lib::db::SettingsStore;
use enhanced_clipboard_lib::i18n::I18n;
use enhanced_clipboard_lib::services::persisted_state::PersistedApp;
use enhanced_clipboard_lib::services::settings::SettingsApp;
use enhanced_clipboard_lib::services::view_events::EventEmitter;

pub struct TestContext {
    pub _tempdir: TempDir,
    pub settings: SettingsStore,
}

impl TestContext {
    pub fn new() -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let settings = SettingsStore::new(
            tempdir
                .path()
                .join("settings.db")
                .to_string_lossy()
                .as_ref(),
        )
        .expect("settings db");
        Self {
            _tempdir: tempdir,
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
        match self
            .autostart_error
            .lock()
            .expect("autostart_error")
            .clone()
        {
            Some(message) => Err(message),
            None => Ok(()),
        }
    }

    fn register_hotkey(&self, hotkey: &str) -> Result<(), String> {
        self.hotkey_calls
            .lock()
            .expect("hotkey_calls")
            .push(hotkey.to_string());
        match self.hotkey_error.lock().expect("hotkey_error").clone() {
            Some(message) => Err(message),
            None => Ok(()),
        }
    }
}

impl PersistedApp for TestApp {
    fn set_always_on_top(&self, enabled: bool) -> Result<(), String> {
        self.always_on_top_calls
            .lock()
            .expect("always_on_top_calls")
            .push(enabled);
        match self
            .always_on_top_error
            .lock()
            .expect("always_on_top_error")
            .clone()
        {
            Some(message) => Err(message),
            None => Ok(()),
        }
    }

    fn restore_window_position(&self, x: i32, y: i32) -> Result<(), String> {
        self.window_position_calls
            .lock()
            .expect("window_position_calls")
            .push((x, y));
        match self
            .window_position_error
            .lock()
            .expect("window_position_error")
            .clone()
        {
            Some(message) => Err(message),
            None => Ok(()),
        }
    }
}
