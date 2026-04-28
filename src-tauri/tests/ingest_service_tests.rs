use std::borrow::Cow;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use arboard::ImageData;

use enhanced_clipboard_lib::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_STREAM_ITEM_ADDED, EVENT_STREAM_ITEM_UPDATED,
};
use enhanced_clipboard_lib::models::{ClipboardListItem, ClipboardPreview};
use enhanced_clipboard_lib::services::ingest::{
    accept_image_clipboard_change, save_image_entry, ImageDedupState,
};
use enhanced_clipboard_lib::utils::image::hash_image_content;

mod common;

use common::{insert_entry, text_entry, TestApp, TestContext};

fn image_data(width: usize, height: usize, bytes: Vec<u8>) -> ImageData<'static> {
    ImageData {
        width,
        height,
        bytes: Cow::Owned(bytes),
    }
}

fn solid_image(width: usize, height: usize, value: u8) -> ImageData<'static> {
    image_data(width, height, vec![value; width * height * 4])
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(condition(), "condition was not met before timeout");
}

#[test]
fn image_hash_detects_different_content_with_same_dimensions_and_size() {
    let first = vec![0u8; 4 * 4 * 4];
    let mut second = first.clone();
    let changed_pixel = (2 * 4 + 1) * 4;
    second[changed_pixel] = 200;

    let first = image_data(4, 4, first);
    let second = image_data(4, 4, second);

    assert_ne!(hash_image_content(&first), hash_image_content(&second));
}

#[test]
fn save_image_entry_emits_pending_then_finalizes_ready_item() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let img = solid_image(2, 2, 255);

    save_image_entry(
        &app,
        &db,
        &ctx.data_dir,
        &img,
        "Photos".to_string(),
        0,
        500,
        None,
    )
    .expect("save image");

    let added = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].thumbnail_path, None);
    assert!(matches!(
        added[0].preview,
        ClipboardPreview::Image {
            mode: enhanced_clipboard_lib::models::ClipboardImagePreviewMode::Pending
        }
    ));
    let id = added[0].id.clone();

    wait_until(|| {
        app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
            .iter()
            .any(|item| item.id == id)
    });

    let entry = db
        .get_entry_by_id(&id)
        .expect("lookup")
        .expect("finalized entry");
    let expected_image = format!("images/{id}.png");
    let expected_thumb = format!("thumbnails/{id}.png");
    assert_eq!(entry.image_path.as_deref(), Some(expected_image.as_str()));
    assert_eq!(
        entry.thumbnail_path.as_deref(),
        Some(expected_thumb.as_str())
    );
    assert_ne!(entry.thumbnail_path, entry.image_path);
    assert!(ctx.data_dir.join(expected_image).exists());
    assert!(ctx.data_dir.join(expected_thumb).exists());
}

#[test]
fn original_write_failure_rolls_back_pending_without_pruning_existing_history() {
    let ctx = TestContext::new();
    let old = text_entry("old", 10, "old");
    insert_entry(&ctx, &old);
    std::fs::remove_dir(ctx.data_dir.join("images")).expect("remove images dir");
    std::fs::write(ctx.data_dir.join("images"), b"not a dir").expect("block images dir");

    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let img = solid_image(2, 2, 64);

    save_image_entry(
        &app,
        &db,
        &ctx.data_dir,
        &img,
        "Photos".to_string(),
        0,
        1,
        None,
    )
    .expect("queue image");
    let id = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)[0]
        .id
        .clone();

    wait_until(|| {
        db.get_entry_by_id(&id).expect("pending lookup").is_none()
            && !app
                .captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED)
                .is_empty()
    });

    assert!(db.get_entry_by_id("old").expect("old lookup").is_some());
}

#[test]
fn image_asset_failure_clears_dedup_hash_so_recapture_is_possible() {
    let ctx = TestContext::new();
    std::fs::remove_dir(ctx.data_dir.join("images")).expect("remove images dir");
    std::fs::write(ctx.data_dir.join("images"), b"not a dir").expect("block images dir");

    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let img = solid_image(2, 2, 64);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));

    let change =
        accept_image_clipboard_change(&app, &db, &ctx.data_dir, &img, "Photos", &dedup, 0, 500)
            .expect("accept")
            .expect("change");
    assert!(change.persist_result.is_ok());
    let id = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)[0]
        .id
        .clone();

    wait_until(|| db.get_entry_by_id(&id).expect("pending lookup").is_none());

    let state = dedup.lock().expect("dedup");
    assert_eq!(state.last_hash, None);
}

#[test]
fn thumbnail_write_failure_rolls_back_pending_and_generated_original() {
    let ctx = TestContext::new();
    std::fs::remove_dir(ctx.data_dir.join("thumbnails")).expect("remove thumbnails dir");
    std::fs::write(ctx.data_dir.join("thumbnails"), b"not a dir").expect("block thumbnails dir");

    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let img = solid_image(601, 301, 180);

    save_image_entry(
        &app,
        &db,
        &ctx.data_dir,
        &img,
        "Photos".to_string(),
        0,
        500,
        None,
    )
    .expect("queue image");
    let id = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)[0]
        .id
        .clone();

    wait_until(|| db.get_entry_by_id(&id).expect("pending lookup").is_none());

    assert!(!ctx.data_dir.join(format!("images/{id}.png")).exists());
}

#[test]
fn finalize_returns_none_after_pending_entry_was_deleted() {
    let ctx = TestContext::new();
    let mut pending = text_entry("pending-image", 10, "");
    pending.content_type = "image".to_string();
    pending.content.clear();
    pending.canonical_search_text.clear();
    insert_entry(&ctx, &pending);

    ctx.db
        .delete_entry("pending-image")
        .expect("delete pending");

    let finalized = ctx
        .db
        .finalize_image_entry(
            "pending-image",
            "images/pending-image.png",
            Some("thumbnails/pending-image.png"),
        )
        .expect("finalize deleted");
    assert!(finalized.is_none());
}

#[test]
fn content_hash_is_recorded_immediately_for_accepted_images() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));
    let img = solid_image(2, 2, 2);

    let change =
        accept_image_clipboard_change(&app, &db, &ctx.data_dir, &img, "Photos", &dedup, 0, 500)
            .expect("accept")
            .expect("change");

    assert!(change.persist_result.is_ok());
    let state = dedup.lock().expect("dedup");
    let expected_hash = hash_image_content(&img);
    assert_eq!(state.last_hash.as_deref(), Some(expected_hash.as_str()));
}

#[test]
fn consecutive_accepts_of_same_image_do_not_duplicate() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState {
        last_hash: Some(hash_image_content(&solid_image(2, 2, 1))),
    }));
    let img = solid_image(2, 2, 2);

    let first =
        accept_image_clipboard_change(&app, &db, &ctx.data_dir, &img, "Photos", &dedup, 0, 500)
            .expect("first accept");
    let second =
        accept_image_clipboard_change(&app, &db, &ctx.data_dir, &img, "Photos", &dedup, 0, 500)
            .expect("second accept");

    assert!(first.is_some());
    assert!(second.is_none());
    assert_eq!(
        app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)
            .len(),
        1
    );
}
