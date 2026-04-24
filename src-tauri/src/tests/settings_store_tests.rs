use crate::constants::{
    DEFAULT_LOG_LEVEL, MAX_HISTORY_ENTRIES, MIN_HISTORY_ENTRIES,
};
use crate::db::SettingsStore;
use crate::models::{AppSettings, PersistedField, PersistedState, SettingsField};

use super::support::TestContext;

#[test]
fn runtime_settings_normalization_clamps_and_sanitizes_values() {
    let normalized = SettingsStore::normalize_runtime_app_settings(&AppSettings {
        hotkey: "  Ctrl+Shift+V  ".to_string(),
        autostart: true,
        max_history: MAX_HISTORY_ENTRIES + 1,
        theme_mode: "SYSTEM".to_string(),
        expiry_seconds: -1,
        capture_images: false,
        log_level: "TRACE".to_string(),
    });

    assert_eq!(normalized.hotkey, "Ctrl+Shift+V");
    assert_eq!(normalized.max_history, MAX_HISTORY_ENTRIES);
    assert_eq!(normalized.theme_mode, "light");
    assert_eq!(normalized.expiry_seconds, 0);
    assert_eq!(normalized.log_level, DEFAULT_LOG_LEVEL);
}

#[test]
fn save_app_settings_fields_updates_only_requested_fields() {
    let ctx = TestContext::new();
    let settings = AppSettings {
        hotkey: "Ctrl+Shift+V".to_string(),
        autostart: true,
        max_history: MIN_HISTORY_ENTRIES,
        theme_mode: "dark".to_string(),
        expiry_seconds: 60,
        capture_images: false,
        log_level: "debug".to_string(),
    };

    ctx.settings
        .save_app_settings_fields(&settings, &[SettingsField::Hotkey, SettingsField::ThemeMode])
        .expect("save app settings");

    let loaded = ctx.settings.load_app_settings().expect("load app settings");
    assert_eq!(loaded.hotkey, "Ctrl+Shift+V");
    assert_eq!(loaded.theme_mode, "dark");
    assert_eq!(loaded.max_history, crate::constants::DEFAULT_MAX_HISTORY);
    assert_eq!(loaded.capture_images, crate::constants::DEFAULT_CAPTURE_IMAGES);
}

#[test]
fn save_persisted_state_fields_supports_optional_coordinate_removal() {
    let ctx = TestContext::new();
    let initial = PersistedState {
        window_x: Some(100),
        window_y: Some(200),
        always_on_top: true,
    };
    ctx.settings
        .save_persisted_state_fields(&initial, &PersistedField::ALL)
        .expect("seed persisted state");

    ctx.settings
        .save_persisted_state_fields(
            &PersistedState {
                window_x: None,
                window_y: Some(240),
                always_on_top: false,
            },
            &[PersistedField::WindowX, PersistedField::WindowY, PersistedField::AlwaysOnTop],
        )
        .expect("update persisted state");

    assert_eq!(
        ctx.settings.load_persisted_state().expect("load persisted state"),
        PersistedState {
            window_x: None,
            window_y: Some(240),
            always_on_top: false,
        }
    );
}
