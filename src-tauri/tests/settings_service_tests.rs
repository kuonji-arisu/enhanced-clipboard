use enhanced_clipboard_lib::constants::{EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE};
use enhanced_clipboard_lib::models::{AppSettingsPatch, ClipboardQueryStaleReason, SettingsField};
use enhanced_clipboard_lib::services::settings::{
    restore_settings_effects, save_settings, WatcherSettingsSink,
};
use std::sync::Mutex;

mod common;

use common::{insert_entry, test_i18n, text_entry, TestApp, TestContext};

#[derive(Default)]
struct TestWatcher {
    refresh_settings_calls: Mutex<Vec<(i64, u32, bool)>>,
    refresh_capture_images_calls: Mutex<Vec<bool>>,
}

impl TestWatcher {
    fn refresh_settings_calls(&self) -> Vec<(i64, u32, bool)> {
        self.refresh_settings_calls
            .lock()
            .expect("refresh_settings_calls")
            .clone()
    }

    fn refresh_capture_images_calls(&self) -> Vec<bool> {
        self.refresh_capture_images_calls
            .lock()
            .expect("refresh_capture_images_calls")
            .clone()
    }
}

impl WatcherSettingsSink for TestWatcher {
    fn refresh_settings(&self, expiry_seconds: i64, max_history: u32, capture_images: bool) {
        self.refresh_settings_calls
            .lock()
            .expect("refresh_settings_calls")
            .push((expiry_seconds, max_history, capture_images));
    }

    fn refresh_capture_images(&self, capture_images: bool) {
        self.refresh_capture_images_calls
            .lock()
            .expect("refresh_capture_images_calls")
            .push(capture_images);
    }
}

#[test]
fn save_settings_prunes_with_retention_and_emits_settings_startup_events() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let watcher = TestWatcher::default();
    let i18n = test_i18n();

    insert_entry(&ctx, &text_entry("expired", 10, "Expired entry"));

    let result = save_settings(
        &app,
        &ctx.db,
        &ctx.settings,
        &watcher,
        &ctx.data_dir,
        &i18n,
        AppSettingsPatch {
            expiry_seconds: Some(1),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert_eq!(result.settings.expiry_seconds, 1);
    assert_eq!(watcher.refresh_settings_calls(), vec![(1, 500, true)]);
    assert!(watcher.refresh_capture_images_calls().is_empty());
    assert_eq!(result.effects.retention.expect("retention effect").ok, true);
    assert!(ctx
        .db
        .get_entry_by_id("expired")
        .expect("expired lookup")
        .is_none());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["expired".to_string()]]
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::SettingsOrStartup]
    );
}

#[test]
fn save_settings_capture_images_isolated_from_retention_side_effects() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let watcher = TestWatcher::default();
    let i18n = test_i18n();

    insert_entry(&ctx, &text_entry("kept", 10, "Still here"));

    let result = save_settings(
        &app,
        &ctx.db,
        &ctx.settings,
        &watcher,
        &ctx.data_dir,
        &i18n,
        AppSettingsPatch {
            capture_images: Some(false),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert_eq!(result.settings.capture_images, false);
    assert!(watcher.refresh_settings_calls().is_empty());
    assert_eq!(watcher.refresh_capture_images_calls(), vec![false]);
    assert!(result.effects.retention.is_none());
    assert_eq!(
        result
            .effects
            .capture_images
            .expect("capture images effect")
            .ok,
        true
    );
    assert!(ctx
        .db
        .get_entry_by_id("kept")
        .expect("kept lookup")
        .is_some());
    assert!(app
        .captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED)
        .is_empty());
    assert!(app
        .captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE)
        .is_empty());
}

#[test]
fn save_settings_persist_then_apply_keeps_saved_intent_when_autostart_effect_fails() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let watcher = TestWatcher::default();
    let i18n = test_i18n();
    app.fail_autostart("autostart manager unavailable");

    let result = save_settings(
        &app,
        &ctx.db,
        &ctx.settings,
        &watcher,
        &ctx.data_dir,
        &i18n,
        AppSettingsPatch {
            autostart: Some(true),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert_eq!(result.settings.autostart, true);
    assert_eq!(
        result.effects.autostart.expect("autostart effect").ok,
        false
    );
    assert_eq!(
        ctx.settings
            .load_app_settings()
            .expect("saved settings")
            .autostart,
        true
    );
    assert_eq!(app.autostart_calls(), vec![true]);
}

#[test]
fn save_settings_persist_then_apply_keeps_saved_intent_when_hotkey_effect_fails() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let watcher = TestWatcher::default();
    let i18n = test_i18n();
    app.fail_hotkey("global shortcut manager unavailable");

    let result = save_settings(
        &app,
        &ctx.db,
        &ctx.settings,
        &watcher,
        &ctx.data_dir,
        &i18n,
        AppSettingsPatch {
            hotkey: Some("Alt+Shift+V".to_string()),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert_eq!(result.settings.hotkey, "Alt+Shift+V");
    assert_eq!(result.effects.hotkey.expect("hotkey effect").ok, false);
    assert_eq!(
        ctx.settings
            .load_app_settings()
            .expect("saved settings")
            .hotkey,
        "Alt+Shift+V"
    );
    assert_eq!(app.hotkey_calls(), vec!["Alt+Shift+V".to_string()]);
}

#[test]
fn restore_settings_effects_refreshes_runtime_settings_and_marks_snapshots_stale() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let watcher = TestWatcher::default();
    let i18n = test_i18n();

    insert_entry(
        &ctx,
        &text_entry("fresh", chrono::Utc::now().timestamp(), "Fresh"),
    );
    ctx.settings
        .save_app_settings_fields(
            &enhanced_clipboard_lib::models::AppSettings {
                hotkey: "CmdOrCtrl+Shift+V".to_string(),
                autostart: false,
                max_history: 600,
                theme_mode: "light".to_string(),
                expiry_seconds: 60,
                capture_images: false,
                log_level: "info".to_string(),
            },
            &[
                SettingsField::Hotkey,
                SettingsField::Autostart,
                SettingsField::MaxHistory,
                SettingsField::ThemeMode,
                SettingsField::ExpirySeconds,
                SettingsField::CaptureImages,
                SettingsField::LogLevel,
            ],
        )
        .expect("seed settings");

    restore_settings_effects(&app, &ctx.db, &ctx.settings, &watcher, &ctx.data_dir, &i18n)
        .expect("restore settings effects");

    assert!(watcher.refresh_settings_calls().contains(&(60, 600, false)));
    assert!(watcher.refresh_capture_images_calls().is_empty());
    assert_eq!(app.autostart_calls(), vec![false]);
    assert_eq!(app.hotkey_calls(), vec!["CmdOrCtrl+Shift+V".to_string()]);
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::SettingsOrStartup]
    );
    assert!(ctx
        .db
        .get_entry_by_id("fresh")
        .expect("fresh lookup")
        .is_some());
}
