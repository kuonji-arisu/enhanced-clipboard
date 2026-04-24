use crate::models::{
    AppSettings, PersistedState, SaveStrategy, SettingsField, SettingsEffectKey,
};

#[test]
fn settings_field_metadata_and_change_detection_remain_centralized() {
    let current = AppSettings::default();
    let next = AppSettings {
        theme_mode: "dark".to_string(),
        max_history: current.max_history + 1,
        ..current.clone()
    };

    assert_eq!(SettingsField::ThemeMode.metadata().strategy, SaveStrategy::PersistOnly);
    assert_eq!(
        SettingsField::MaxHistory.metadata().effect,
        Some(SettingsEffectKey::Retention)
    );
    assert!(SettingsField::ThemeMode.changed(&current, &next));
    assert!(SettingsField::MaxHistory.changed(&current, &next));
    assert!(!SettingsField::Hotkey.changed(&current, &next));
}

#[test]
fn persisted_state_change_detection_tracks_optional_coordinates() {
    let current = PersistedState {
        window_x: Some(10),
        window_y: Some(20),
        always_on_top: false,
    };
    let next = PersistedState {
        window_x: None,
        window_y: Some(20),
        always_on_top: true,
    };

    assert!(crate::models::PersistedField::WindowX.changed(&current, &next));
    assert!(!crate::models::PersistedField::WindowY.changed(&current, &next));
    assert!(crate::models::PersistedField::AlwaysOnTop.changed(&current, &next));
}
