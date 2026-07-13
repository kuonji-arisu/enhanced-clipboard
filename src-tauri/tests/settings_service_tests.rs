use enhanced_clipboard_lib::clipboard::ClipboardPolicy;
use enhanced_clipboard_lib::models::{AppSettings, AppSettingsPatch, SettingsField};
use enhanced_clipboard_lib::services::settings::{
    restore_settings_effects, save_settings, ClipboardPolicySink,
};
use std::sync::Mutex;

mod common;

use common::{test_i18n, TestApp, TestContext};

#[derive(Default)]
struct TestPolicySink {
    calls: Mutex<Vec<(i64, u32, bool)>>,
    error: Mutex<Option<String>>,
}

impl TestPolicySink {
    fn calls(&self) -> Vec<(i64, u32, bool)> {
        self.calls.lock().expect("policy calls").clone()
    }

    fn fail(&self, message: impl Into<String>) {
        *self.error.lock().expect("policy error") = Some(message.into());
    }
}

impl ClipboardPolicySink for TestPolicySink {
    fn apply_policy(&self, policy: ClipboardPolicy) -> Result<(), String> {
        self.calls.lock().expect("policy calls").push((
            policy.expiry_seconds,
            policy.max_history,
            policy.capture_images,
        ));
        match self.error.lock().expect("policy error").clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[test]
fn save_settings_retention_applies_the_complete_clipboard_policy() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let clipboard = TestPolicySink::default();
    let i18n = test_i18n();

    let result = save_settings(
        &app,
        &ctx.settings,
        &clipboard,
        &i18n,
        AppSettingsPatch {
            expiry_seconds: Some(1),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert_eq!(result.settings.expiry_seconds, 1);
    assert_eq!(clipboard.calls(), vec![(1, 200, true)]);
    assert!(result.effects.retention.expect("retention effect").ok);
    assert!(result.effects.capture_images.is_none());
}

#[test]
fn save_settings_capture_images_applies_the_complete_clipboard_policy() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let clipboard = TestPolicySink::default();
    let i18n = test_i18n();

    let result = save_settings(
        &app,
        &ctx.settings,
        &clipboard,
        &i18n,
        AppSettingsPatch {
            capture_images: Some(false),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert!(!result.settings.capture_images);
    assert_eq!(clipboard.calls(), vec![(0, 200, false)]);
    assert!(result.effects.retention.is_none());
    assert!(
        result
            .effects
            .capture_images
            .expect("capture images effect")
            .ok
    );
}

#[test]
fn save_settings_multiple_policy_effects_submit_the_same_complete_policy() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let clipboard = TestPolicySink::default();
    let i18n = test_i18n();

    let result = save_settings(
        &app,
        &ctx.settings,
        &clipboard,
        &i18n,
        AppSettingsPatch {
            max_history: Some(600),
            capture_images: Some(false),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert_eq!(clipboard.calls(), vec![(0, 600, false), (0, 600, false)]);
    assert!(result.effects.retention.expect("retention effect").ok);
    assert!(
        result
            .effects
            .capture_images
            .expect("capture images effect")
            .ok
    );
}

#[test]
fn save_settings_policy_failure_keeps_the_saved_intent() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let clipboard = TestPolicySink::default();
    let i18n = test_i18n();
    clipboard.fail("engine unavailable");

    let result = save_settings(
        &app,
        &ctx.settings,
        &clipboard,
        &i18n,
        AppSettingsPatch {
            expiry_seconds: Some(60),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert_eq!(result.settings.expiry_seconds, 60);
    let effect = result.effects.retention.expect("retention effect");
    assert!(!effect.ok);
    assert!(effect
        .error
        .expect("effect error")
        .contains("engine unavailable"));
    assert_eq!(
        ctx.settings
            .load_app_settings()
            .expect("saved settings")
            .expiry_seconds,
        60
    );
}

#[test]
fn save_settings_persist_then_apply_keeps_saved_intent_when_autostart_effect_fails() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let clipboard = TestPolicySink::default();
    let i18n = test_i18n();
    app.fail_autostart("autostart manager unavailable");

    let result = save_settings(
        &app,
        &ctx.settings,
        &clipboard,
        &i18n,
        AppSettingsPatch {
            autostart: Some(true),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert!(result.settings.autostart);
    assert!(!result.effects.autostart.expect("autostart effect").ok);
    assert!(
        ctx.settings
            .load_app_settings()
            .expect("saved settings")
            .autostart
    );
    assert_eq!(app.autostart_calls(), vec![true]);
    assert!(clipboard.calls().is_empty());
}

#[test]
fn save_settings_persist_then_apply_keeps_saved_intent_when_hotkey_effect_fails() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let clipboard = TestPolicySink::default();
    let i18n = test_i18n();
    app.fail_hotkey("global shortcut manager unavailable");

    let result = save_settings(
        &app,
        &ctx.settings,
        &clipboard,
        &i18n,
        AppSettingsPatch {
            hotkey: Some("Alt+Shift+V".to_string()),
            ..AppSettingsPatch::default()
        },
    )
    .expect("save settings");

    assert_eq!(result.settings.hotkey, "Alt+Shift+V");
    assert!(!result.effects.hotkey.expect("hotkey effect").ok);
    assert_eq!(
        ctx.settings
            .load_app_settings()
            .expect("saved settings")
            .hotkey,
        "Alt+Shift+V"
    );
    assert_eq!(app.hotkey_calls(), vec!["Alt+Shift+V".to_string()]);
    assert!(clipboard.calls().is_empty());
}

#[test]
fn restore_settings_effects_applies_the_complete_policy_once() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let clipboard = TestPolicySink::default();
    let i18n = test_i18n();

    ctx.settings
        .save_app_settings_fields(
            &AppSettings {
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

    restore_settings_effects(&app, &ctx.settings, &clipboard, &i18n)
        .expect("restore settings effects");

    assert_eq!(clipboard.calls(), vec![(60, 600, false)]);
    assert_eq!(app.autostart_calls(), vec![false]);
    assert_eq!(app.hotkey_calls(), vec!["CmdOrCtrl+Shift+V".to_string()]);
}
