use crate::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_UPDATED,
};
use crate::models::{AppSettings, ClipboardListItem, ClipboardQueryStaleReason, SettingsField};
use crate::services::entry::{
    clear_all_entries, handle_image_load_failed, remove_entry, toggle_pin_entry,
};

use super::support::{
    image_entry, insert_entry, test_i18n, text_entry, touch_file, TestApp, TestContext,
};

#[test]
fn remove_entry_and_clear_all_delete_associated_asset_files() {
    let ctx = TestContext::new();
    let first = image_entry("first", 10);
    let second = image_entry("second", 11);
    touch_file(&ctx, first.image_path.as_deref().expect("first image"));
    touch_file(&ctx, first.thumbnail_path.as_deref().expect("first thumb"));
    touch_file(&ctx, second.image_path.as_deref().expect("second image"));
    touch_file(
        &ctx,
        second.thumbnail_path.as_deref().expect("second thumb"),
    );
    insert_entry(&ctx, &first);
    insert_entry(&ctx, &second);

    assert!(remove_entry(&ctx.db, &ctx.data_dir, "first").expect("remove entry"));
    assert!(ctx
        .db
        .get_entry_by_id("first")
        .expect("first lookup")
        .is_none());
    assert!(!ctx
        .data_dir
        .join(first.image_path.as_deref().expect("first image"))
        .exists());

    let cleared_ids = clear_all_entries(&ctx.db, &ctx.data_dir).expect("clear all");
    assert_eq!(cleared_ids, vec!["second".to_string()]);
    assert!(ctx
        .db
        .get_entry_by_id("second")
        .expect("second lookup")
        .is_none());
    assert!(!ctx
        .data_dir
        .join(second.thumbnail_path.as_deref().expect("second thumb"))
        .exists());
}

#[test]
fn broken_image_reports_remove_only_image_entries() {
    let ctx = TestContext::new();
    let image = image_entry("image", 10);
    touch_file(&ctx, image.image_path.as_deref().expect("image path"));
    touch_file(&ctx, image.thumbnail_path.as_deref().expect("thumb path"));
    insert_entry(&ctx, &image);
    insert_entry(&ctx, &text_entry("text", 11, "Alpha"));

    assert!(handle_image_load_failed(&ctx.db, &ctx.data_dir, "image").expect("image failure"));
    assert!(!ctx
        .data_dir
        .join(image.image_path.as_deref().expect("image path"))
        .exists());
    assert!(!ctx
        .data_dir
        .join(image.thumbnail_path.as_deref().expect("thumb path"))
        .exists());
    assert!(!handle_image_load_failed(&ctx.db, &ctx.data_dir, "image").expect("repeat failure"));
    assert!(!handle_image_load_failed(&ctx.db, &ctx.data_dir, "text").expect("text failure"));
}

#[test]
fn toggle_pin_emits_stream_update_and_pin_changed_when_entry_remains_visible() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let i18n = test_i18n();

    insert_entry(
        &ctx,
        &text_entry("entry", chrono::Utc::now().timestamp(), "Alpha"),
    );

    toggle_pin_entry(
        &app,
        &ctx.db,
        &ctx.settings,
        &ctx.data_dir,
        "entry",
        &i18n.read().expect("i18n"),
    )
    .expect("toggle pin");

    assert_eq!(
        ctx.db
            .get_entry_by_id("entry")
            .expect("entry lookup")
            .unwrap()
            .is_pinned,
        true
    );
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        Vec::<Vec<String>>::new()
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::PinChanged]
    );
    let updated_payloads = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert_eq!(updated_payloads.len(), 1);
    assert_eq!(updated_payloads[0].id, "entry");
    assert_eq!(updated_payloads[0].is_pinned, true);
}

#[test]
fn toggle_pin_unpin_retention_emits_removed_entries_instead_of_stream_update() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let i18n = test_i18n();

    ctx.settings
        .save_app_settings_fields(
            &AppSettings {
                hotkey: "CmdOrCtrl+Shift+V".to_string(),
                autostart: false,
                max_history: 500,
                theme_mode: "light".to_string(),
                expiry_seconds: 1,
                capture_images: true,
                log_level: "error".to_string(),
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

    let mut entry = text_entry("pinned-old", 10, "Old pinned");
    entry.is_pinned = true;
    insert_entry(&ctx, &entry);

    toggle_pin_entry(
        &app,
        &ctx.db,
        &ctx.settings,
        &ctx.data_dir,
        "pinned-old",
        &i18n.read().expect("i18n"),
    )
    .expect("toggle pin");

    assert!(ctx
        .db
        .get_entry_by_id("pinned-old")
        .expect("entry lookup")
        .is_none());
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["pinned-old".to_string()]]
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::UnpinRetention]
    );
}
