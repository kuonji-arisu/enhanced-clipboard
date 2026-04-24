use crate::constants::EVENT_RUNTIME_STATUS_UPDATED;
use crate::models::{RuntimeStatusPatch, RuntimeStatusState};
use crate::services::runtime::{apply_patch, initial_status};

use super::support::TestApp;

#[test]
fn apply_patch_updates_runtime_snapshot_and_emits_only_changed_fields() {
    let app = TestApp::new();
    let state = RuntimeStatusState(std::sync::Mutex::new(initial_status()));

    let snapshot = apply_patch(
        &app,
        &state,
        RuntimeStatusPatch {
            clipboard_capture_available: Some(false),
            system_theme: Some("dark".to_string()),
        },
    )
    .expect("apply patch");

    assert_eq!(snapshot.clipboard_capture_available, false);
    assert_eq!(snapshot.system_theme, "dark");
    assert_eq!(
        app.captured_event::<RuntimeStatusPatch>(EVENT_RUNTIME_STATUS_UPDATED),
        vec![RuntimeStatusPatch {
            clipboard_capture_available: Some(false),
            system_theme: Some("dark".to_string()),
        }]
    );

    let same_snapshot = apply_patch(
        &app,
        &state,
        RuntimeStatusPatch {
            clipboard_capture_available: Some(false),
            system_theme: Some("dark".to_string()),
        },
    )
    .expect("apply identical patch");

    assert_eq!(same_snapshot.clipboard_capture_available, false);
    assert_eq!(same_snapshot.system_theme, "dark");
    assert_eq!(
        app.captured_event::<RuntimeStatusPatch>(EVENT_RUNTIME_STATUS_UPDATED)
            .len(),
        1
    );
}
