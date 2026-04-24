use crate::models::{PersistedState, PersistedStatePatch, PersistedField};
use crate::services::persisted_state::{restore_persisted_effects, save_persisted};

use super::support::{test_i18n, TestApp, TestContext};

#[test]
fn save_persisted_keeps_db_value_when_always_on_top_effect_cannot_run() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    app.fail_always_on_top("Main window not found");
    let i18n = test_i18n();

    let result = save_persisted(
        &app,
        &ctx.settings,
        &i18n,
        PersistedStatePatch {
            always_on_top: Some(true),
            ..PersistedStatePatch::default()
        },
    )
    .expect("save persisted");

    assert_eq!(
        result.persisted,
        PersistedState {
            window_x: None,
            window_y: None,
            always_on_top: false,
        }
    );
    assert_eq!(
        result
            .effects
            .expect("effects")
            .always_on_top
            .expect("always_on_top")
            .ok,
        false
    );
    assert_eq!(
        ctx.settings.load_persisted_state().expect("persisted state"),
        PersistedState::default()
    );
}

#[test]
fn save_persisted_applies_then_persists_always_on_top_with_window_state_updates() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let i18n = test_i18n();

    let result = save_persisted(
        &app,
        &ctx.settings,
        &i18n,
        PersistedStatePatch {
            window_x: Some(Some(120)),
            window_y: Some(Some(240)),
            always_on_top: Some(true),
        },
    )
    .expect("save persisted");

    assert_eq!(
        result.persisted,
        PersistedState {
            window_x: Some(120),
            window_y: Some(240),
            always_on_top: true,
        }
    );
    assert_eq!(
        result
            .effects
            .expect("effects")
            .always_on_top
            .expect("always_on_top")
            .ok,
        true
    );
    assert_eq!(
        ctx.settings.load_persisted_state().expect("persisted state"),
        PersistedState {
            window_x: Some(120),
            window_y: Some(240),
            always_on_top: true,
        }
    );
    assert_eq!(app.always_on_top_calls(), vec![true]);
}

#[test]
fn restore_persisted_effects_handles_saved_window_state_with_main_window_present() {
    let ctx = TestContext::new();
    let app = TestApp::new();

    ctx.settings
        .save_persisted_state_fields(
            &PersistedState {
                window_x: Some(10),
                window_y: Some(20),
                always_on_top: true,
            },
            &PersistedField::ALL,
        )
        .expect("seed persisted state");

    restore_persisted_effects(&app, &ctx.settings).expect("restore persisted effects");

    assert_eq!(
        ctx.settings.load_persisted_state().expect("persisted state"),
        PersistedState {
            window_x: Some(10),
            window_y: Some(20),
            always_on_top: true,
        }
    );
    assert_eq!(app.window_position_calls(), vec![(10, 20)]);
    assert_eq!(app.always_on_top_calls(), vec![true]);
}

#[test]
fn restore_persisted_effects_tolerates_missing_coordinates_and_runtime_failures() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    app.fail_window_position("window not ready");
    app.fail_always_on_top("main window not found");

    ctx.settings
        .save_persisted_state_fields(
            &PersistedState {
                window_x: Some(10),
                window_y: None,
                always_on_top: true,
            },
            &PersistedField::ALL,
        )
        .expect("seed persisted state");

    restore_persisted_effects(&app, &ctx.settings).expect("restore persisted effects");

    assert!(app.window_position_calls().is_empty());
    assert_eq!(app.always_on_top_calls(), vec![true]);
    assert_eq!(
        ctx.settings.load_persisted_state().expect("persisted state"),
        PersistedState {
            window_x: Some(10),
            window_y: None,
            always_on_top: true,
        }
    );
}
