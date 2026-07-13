use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Local;
use enhanced_clipboard_lib::clipboard::{
    CapturedPayload, ClipboardChanged, ClipboardContentType, ClipboardEngine,
    ClipboardEngineConfig, ClipboardEngineHandle, ClipboardEntriesQuery, ClipboardError,
    ClipboardListItem, ClipboardListPage, ClipboardPolicy, ClipboardPreview, ClipboardWriter,
    ImagePreviewRepairOutcome,
};
use rusqlite::Connection;
use tempfile::TempDir;

const DATABASE_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
type RecordedEvents = Arc<Mutex<Vec<ClipboardChanged>>>;

#[derive(Debug, Default)]
struct WriterCalls {
    texts: Vec<String>,
    images: Vec<(Vec<u8>, u32, u32)>,
}

struct RecordingWriter {
    calls: Arc<Mutex<WriterCalls>>,
    stopped: Arc<AtomicBool>,
}

impl Drop for RecordingWriter {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

impl ClipboardWriter for RecordingWriter {
    fn write_text(&mut self, text: &str) -> Result<(), String> {
        self.calls.lock().unwrap().texts.push(text.to_string());
        Ok(())
    }

    fn write_image(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .images
            .push((rgba.to_vec(), width, height));
        Ok(())
    }
}

struct TestWorkspace {
    root: TempDir,
}

impl TestWorkspace {
    fn new() -> Self {
        let workspace = Self {
            root: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(workspace.data_dir()).unwrap();
        workspace
    }

    fn data_dir(&self) -> PathBuf {
        self.root.path().join("data")
    }

    fn database_path(&self) -> PathBuf {
        self.data_dir().join("clipboard.db")
    }
}

struct RunningEngine {
    engine: Option<ClipboardEngine>,
    handle: Option<ClipboardEngineHandle>,
    writer_calls: Arc<Mutex<WriterCalls>>,
    stopped: Arc<AtomicBool>,
}

impl RunningEngine {
    fn handle(&self) -> &ClipboardEngineHandle {
        self.handle.as_ref().unwrap()
    }

    fn stop(mut self) {
        self.handle.take();
        self.engine.take();
        wait_until("clipboard engine thread to stop", || {
            self.stopped.load(Ordering::Acquire)
        });
    }
}

fn start_engine(
    workspace: &TestWorkspace,
    policy: ClipboardPolicy,
    fail_events: bool,
) -> (RunningEngine, RecordedEvents) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured_events = Arc::clone(&events);
    let writer_calls = Arc::new(Mutex::new(WriterCalls::default()));
    let stopped = Arc::new(AtomicBool::new(false));
    let config = ClipboardEngineConfig::new(
        workspace.database_path(),
        DATABASE_KEY,
        false,
        workspace.data_dir(),
        policy,
        move |event| {
            if fail_events {
                Err("test event sink failure".to_string())
            } else {
                captured_events.lock().unwrap().push(event);
                Ok(())
            }
        },
    )
    .with_clipboard_writer(RecordingWriter {
        calls: Arc::clone(&writer_calls),
        stopped: Arc::clone(&stopped),
    });
    let engine = ClipboardEngine::start(config).unwrap();
    let handle = engine.handle();
    (
        RunningEngine {
            engine: Some(engine),
            handle: Some(handle),
            writer_calls,
            stopped,
        },
        events,
    )
}

fn policy(max_history: u32) -> ClipboardPolicy {
    ClipboardPolicy {
        expiry_seconds: 0,
        max_history,
        capture_images: true,
    }
}

fn capture_text(handle: &ClipboardEngineHandle, text: impl Into<String>) {
    capture(
        handle,
        CapturedPayload::Text {
            content: text.into(),
            source_app: "test.exe".to_string(),
        },
    );
}

fn capture_image(handle: &ClipboardEngineHandle, rgba: Vec<u8>, width: u32, height: u32) {
    capture(
        handle,
        CapturedPayload::Image {
            rgba,
            width,
            height,
            source_app: "snippingtool.exe".to_string(),
        },
    );
}

fn capture(handle: &ClipboardEngineHandle, mut payload: CapturedPayload) {
    let deadline = Instant::now() + POLL_TIMEOUT;
    loop {
        match handle.try_capture(payload) {
            Ok(()) => return,
            Err(std::sync::mpsc::TrySendError::Full(returned)) => {
                assert!(Instant::now() < deadline, "clipboard mailbox stayed full");
                payload = returned;
                std::thread::yield_now();
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                panic!("clipboard engine disconnected while capturing")
            }
        }
    }
}

fn wait_for_page(
    handle: &ClipboardEngineHandle,
    description: &str,
    predicate: impl Fn(&ClipboardListPage) -> bool,
) -> ClipboardListPage {
    let deadline = Instant::now() + POLL_TIMEOUT;
    loop {
        let page = handle.list(ClipboardEntriesQuery::default()).unwrap();
        if predicate(&page) {
            return page;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}; last page: {page:?}"
        );
        std::thread::yield_now();
    }
}

fn wait_until(description: &str, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + POLL_TIMEOUT;
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        std::thread::yield_now();
    }
}

fn list_all_with_small_pages(handle: &ClipboardEngineHandle) -> ClipboardListPage {
    let mut cursor = None;
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut expected_revision = None;
    let mut expected_pinned_count = None;

    loop {
        let page = handle
            .list(ClipboardEntriesQuery {
                cursor: cursor.clone(),
                limit: Some(3),
                ..Default::default()
            })
            .unwrap();
        if let Some(revision) = expected_revision {
            assert_eq!(page.revision, revision);
        } else {
            expected_revision = Some(page.revision);
        }
        if let Some(pinned_count) = expected_pinned_count {
            assert_eq!(page.pinned_count, pinned_count);
        } else {
            expected_pinned_count = Some(page.pinned_count);
        }
        if cursor.is_some() {
            assert!(page.items.iter().all(|item| !item.is_pinned));
        }
        for item in page.items {
            assert!(
                seen.insert(item.id.clone()),
                "cursor returned a duplicate id"
            );
            items.push(item);
        }

        let Some(next_cursor) = page.next_cursor else {
            break;
        };
        cursor = Some(next_cursor);
    }

    ClipboardListPage {
        revision: expected_revision.unwrap(),
        items,
        next_cursor: None,
        pinned_count: expected_pinned_count.unwrap(),
    }
}

fn assert_page_invariants(page: &ClipboardListPage, max_history: u32) {
    let pinned = page
        .items
        .iter()
        .filter(|item| item.is_pinned)
        .collect::<Vec<_>>();
    let normal = page
        .items
        .iter()
        .filter(|item| !item.is_pinned)
        .collect::<Vec<_>>();

    assert_eq!(pinned.len(), page.pinned_count as usize);
    assert!(page.pinned_count <= 3);
    assert!(normal.len() <= max_history as usize);
    assert!(page
        .items
        .iter()
        .take(pinned.len())
        .all(|item| item.is_pinned));
    assert!(page
        .items
        .iter()
        .skip(pinned.len())
        .all(|item| !item.is_pinned));

    for partition in [&pinned, &normal] {
        assert!(partition.windows(2).all(|pair| {
            (pair[0].created_at, pair[0].id.as_str()) >= (pair[1].created_at, pair[1].id.as_str())
        }));
    }
    assert!(page.items.iter().all(|item| matches!(
        item.content_type,
        ClipboardContentType::Text | ClipboardContentType::Image
    )));
}

fn next_seed(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed
}

fn preview_text(item: &ClipboardListItem) -> &str {
    match &item.preview {
        ClipboardPreview::Text { text, .. } => text,
        ClipboardPreview::Image { .. } => panic!("expected a text preview"),
    }
}

fn direct_children(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}

fn open_encrypted_database(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(&format!("PRAGMA key = \"x'{DATABASE_KEY}'\";"))
        .unwrap();
    connection
}

#[test]
fn text_capture_dedup_and_copy_suppression_are_engine_owned() {
    let workspace = TestWorkspace::new();
    let (running, events) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    capture_text(handle, "alpha");
    let first = wait_for_page(handle, "first capture", |page| page.items.len() == 1);
    assert_eq!(first.revision, 1);

    capture_text(handle, "alpha");
    let duplicate = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert_eq!(duplicate.items.len(), 1);
    assert_eq!(duplicate.revision, first.revision);

    capture_text(handle, "beta");
    let two_items = wait_for_page(handle, "second distinct capture", |page| {
        page.items.len() == 2
    });
    let alpha_id = two_items
        .items
        .iter()
        .find(|item| preview_text(item) == "alpha")
        .unwrap()
        .id
        .clone();
    handle.copy(alpha_id).unwrap();
    assert_eq!(running.writer_calls.lock().unwrap().texts, ["alpha"]);

    // The watcher notification caused by copying the historical alpha entry
    // must not insert it again even though beta was the last captured entry.
    capture_text(handle, "alpha");
    let suppressed = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert_eq!(suppressed.items.len(), 2);
    assert_eq!(suppressed.revision, two_items.revision);

    let recorded = events.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].revision, 1);
    assert_eq!(recorded[1].revision, 2);
}

#[test]
fn dedup_is_consecutive_across_payload_types_and_disabled_images_break_the_sequence() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    capture_text(handle, "alpha");
    capture_image(handle, vec![1, 2, 3, 255], 1, 1);
    capture_text(handle, "alpha");
    let separated = wait_for_page(handle, "cross-type captures", |page| page.items.len() == 3);
    assert_eq!(
        separated
            .items
            .iter()
            .filter(|item| matches!(&item.preview, ClipboardPreview::Text { text, .. } if text == "alpha"))
            .count(),
        2
    );

    handle
        .apply_policy(ClipboardPolicy {
            capture_images: false,
            ..policy(20)
        })
        .unwrap();
    capture_text(handle, "omega");
    capture_image(handle, vec![9, 8, 7, 255], 1, 1);
    capture_text(handle, "omega");
    let disabled_separator = wait_for_page(handle, "disabled image separator", |page| {
        page.items.len() == 5
    });
    assert_eq!(
        disabled_separator
            .items
            .iter()
            .filter(|item| matches!(&item.preview, ClipboardPreview::Text { text, .. } if text == "omega"))
            .count(),
        2
    );
}

#[test]
fn query_filters_and_backend_projection_are_strict() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    capture_text(handle, "https://Example.com/path");
    capture_text(handle, r#"{"answer":42}"#);
    capture_text(handle, "plain note");
    capture_image(handle, vec![40, 50, 60, 255], 1, 1);
    wait_for_page(handle, "query fixtures", |page| page.items.len() == 4);

    let text_match = handle
        .list(ClipboardEntriesQuery {
            text: Some("EXAMPLE".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(text_match.items.len(), 1);
    assert!(matches!(
        &text_match.items[0].preview,
        ClipboardPreview::Text {
            highlight_ranges,
            ..
        } if !highlight_ranges.is_empty()
    ));

    let url_tag = handle
        .list(ClipboardEntriesQuery {
            tag: Some("url".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(url_tag.items.len(), 1);
    assert_eq!(url_tag.items[0].tags, ["url"]);
    let json_tag = handle
        .list(ClipboardEntriesQuery {
            tag: Some("json".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(json_tag.items.len(), 1);

    let images = handle
        .list(ClipboardEntriesQuery {
            entry_type: Some(ClipboardContentType::Image),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(images.items.len(), 1);
    assert_eq!(images.items[0].content_type, ClipboardContentType::Image);

    let today = Local::now().format("%Y-%m-%d").to_string();
    let today_page = handle
        .list(ClipboardEntriesQuery {
            date: Some(today),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(today_page.items.len(), 4);
    let impossible_date = handle
        .list(ClipboardEntriesQuery {
            date: Some("1900-01-01".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert!(impossible_date.items.is_empty());
}

#[test]
fn pin_limit_and_unpin_retention_are_atomic() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    for value in ["one", "two", "three", "four"] {
        capture_text(handle, value);
    }
    let initial = wait_for_page(handle, "pin candidates", |page| page.items.len() == 4);
    for item in initial.items.iter().take(3) {
        handle.toggle_pin(&item.id).unwrap();
    }
    let fourth = initial.items[3].id.clone();
    assert!(matches!(
        handle.toggle_pin(&fourth),
        Err(ClipboardError::PinLimitExceeded { limit: 3 })
    ));

    handle.apply_policy(policy(0)).unwrap();
    let pinned_only = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert_eq!(pinned_only.items.len(), 3);
    assert!(pinned_only.items.iter().all(|item| item.is_pinned));

    let unpinned_id = pinned_only.items[0].id.clone();
    handle.toggle_pin(unpinned_id).unwrap();
    let pruned = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert_eq!(pruned.items.len(), 2);
    assert_eq!(pruned.pinned_count, 2);
    assert!(pruned.items.iter().all(|item| item.is_pinned));
}

#[test]
fn expiry_projection_change_is_a_revision_even_when_prune_is_a_no_op() {
    let workspace = TestWorkspace::new();
    let (running, events) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    capture_text(handle, "ttl projection");
    let before = wait_for_page(handle, "ttl projection entry", |page| page.items.len() == 1);
    assert_eq!(before.revision, 1);
    assert_eq!(before.items[0].visible_until, None);

    handle
        .apply_policy(ClipboardPolicy {
            expiry_seconds: 3_600,
            ..policy(20)
        })
        .unwrap();
    let after = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert_eq!(after.revision, 2);
    assert!(after.items[0].visible_until.is_some());
    assert_eq!(events.lock().unwrap().len(), 2);

    handle
        .apply_policy(ClipboardPolicy {
            expiry_seconds: 3_600,
            ..policy(20)
        })
        .unwrap();
    assert_eq!(
        handle
            .list(ClipboardEntriesQuery::default())
            .unwrap()
            .revision,
        2
    );
}

#[test]
fn pinned_first_cursor_retention_and_clear_share_one_ordered_state() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    for text in ["one", "two", "three", "four", "five"] {
        capture_text(handle, text);
    }
    let all = wait_for_page(handle, "five captures", |page| page.items.len() == 5);
    let pinned_id = all
        .items
        .iter()
        .find(|item| preview_text(item) == "three")
        .unwrap()
        .id
        .clone();
    handle.toggle_pin(&pinned_id).unwrap();

    let first_page = handle
        .list(ClipboardEntriesQuery {
            limit: Some(2),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first_page.pinned_count, 1);
    assert_eq!(first_page.items.len(), 3);
    assert!(first_page.items[0].is_pinned);
    assert_eq!(first_page.items[0].id, pinned_id);
    assert!(first_page.items[1..].iter().all(|item| !item.is_pinned));
    let cursor = first_page.next_cursor.clone().unwrap();
    let first_ids = first_page
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();

    let second_page = handle
        .list(ClipboardEntriesQuery {
            cursor: Some(cursor),
            limit: Some(2),
            ..Default::default()
        })
        .unwrap();
    assert!(second_page.items.iter().all(|item| !item.is_pinned));
    assert!(second_page
        .items
        .iter()
        .all(|item| !first_ids.contains(&item.id)));
    assert_eq!(second_page.pinned_count, 1);

    let filtered = handle
        .list(ClipboardEntriesQuery {
            text: Some("does-not-match".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert!(filtered.items.is_empty());
    assert_eq!(filtered.pinned_count, 1);

    handle.apply_policy(policy(2)).unwrap();
    let retained = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert_eq!(retained.items.len(), 3);
    assert_eq!(
        retained.items.iter().filter(|item| item.is_pinned).count(),
        1
    );
    assert_eq!(retained.pinned_count, 1);

    handle.clear().unwrap();
    let cleared = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert!(cleared.items.is_empty());
    assert_eq!(cleared.pinned_count, 0);
}

#[test]
fn ttl_hides_only_non_pinned_entries() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    capture_text(handle, "pinned survives ttl");
    let first = wait_for_page(handle, "entry to pin", |page| page.items.len() == 1);
    handle.toggle_pin(&first.items[0].id).unwrap();
    capture_text(handle, "expires");
    let ready = wait_for_page(handle, "both ttl entries", |page| page.items.len() == 2);
    handle
        .apply_policy(ClipboardPolicy {
            expiry_seconds: 1,
            max_history: 20,
            capture_images: true,
        })
        .unwrap();
    assert_eq!(ready.pinned_count, 1);

    let expired = wait_for_page(handle, "non-pinned ttl expiration", |page| {
        page.items.len() == 1 && page.items[0].is_pinned
    });
    assert_eq!(preview_text(&expired.items[0]), "pinned survives ttl");
    assert_eq!(expired.pinned_count, 1);
}

#[test]
fn image_rows_are_ready_only_and_preview_repair_removes_broken_originals() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    capture_image(handle, vec![0; 3], 1, 1);
    let invalid = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert!(invalid.items.is_empty());
    assert!(direct_children(&workspace.data_dir().join("images")).is_empty());
    assert!(direct_children(&workspace.data_dir().join("thumbnails")).is_empty());

    let rgba = vec![
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ];
    capture_image(handle, rgba.clone(), 2, 2);
    let ready = wait_for_page(handle, "ready image", |page| page.items.len() == 1);
    let item = &ready.items[0];
    assert_eq!(item.content_type, ClipboardContentType::Image);
    assert!(matches!(
        item.preview,
        ClipboardPreview::Image { src: Some(_) }
    ));
    let original = workspace
        .data_dir()
        .join("images")
        .join(format!("{}.png", item.id));
    let preview = direct_children(&workspace.data_dir().join("thumbnails"))
        .into_iter()
        .find(|path| path.file_stem().unwrap() == item.id.as_str())
        .unwrap();
    assert!(original.is_file());
    assert!(preview.is_file());
    assert!(direct_children(&workspace.data_dir().join("images"))
        .iter()
        .chain(direct_children(&workspace.data_dir().join("thumbnails")).iter())
        .all(|path| !path.file_name().unwrap().to_string_lossy().contains(".tmp")));

    handle.copy(&item.id).unwrap();
    assert_eq!(running.writer_calls.lock().unwrap().images, [(rgba, 2, 2)]);

    std::fs::remove_file(&preview).unwrap();
    let missing_preview = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert!(matches!(
        missing_preview.items[0].preview,
        ClipboardPreview::Image { src: None }
    ));
    assert_eq!(
        handle.repair_preview(&item.id).unwrap(),
        ImagePreviewRepairOutcome::Repaired
    );
    let repaired_previews = direct_children(&workspace.data_dir().join("thumbnails"));
    assert_eq!(repaired_previews.len(), 1);
    assert!(repaired_previews[0].is_file());

    std::fs::remove_file(original).unwrap();
    assert_eq!(
        handle.repair_preview(&item.id).unwrap(),
        ImagePreviewRepairOutcome::Removed
    );
    let removed = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert!(removed.items.is_empty());
    assert!(direct_children(&workspace.data_dir().join("thumbnails")).is_empty());

    let second_rgba = vec![7, 8, 9, 255];
    capture_image(handle, second_rgba, 1, 1);
    let second = wait_for_page(handle, "second ready image", |page| page.items.len() == 1);
    let second_id = second.items[0].id.clone();
    std::fs::remove_file(
        workspace
            .data_dir()
            .join("images")
            .join(format!("{second_id}.png")),
    )
    .unwrap();
    assert!(matches!(
        handle.copy(&second_id),
        Err(ClipboardError::ImageOriginalUnavailable)
    ));
    assert!(handle
        .list(ClipboardEntriesQuery::default())
        .unwrap()
        .items
        .is_empty());
    assert!(direct_children(&workspace.data_dir().join("thumbnails")).is_empty());
}

#[test]
fn failed_event_delivery_does_not_rollback_or_inflate_revision() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), true);
    let handle = running.handle();

    capture_text(handle, "committed despite event failure");
    let committed = wait_for_page(handle, "capture with failed event", |page| {
        page.items.len() == 1
    });
    assert_eq!(committed.revision, 1);

    capture_text(handle, "committed despite event failure");
    handle.apply_policy(policy(20)).unwrap();
    let no_op = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert_eq!(no_op.items.len(), 1);
    assert_eq!(no_op.revision, 1);

    handle.clear().unwrap();
    let cleared = handle.list(ClipboardEntriesQuery::default()).unwrap();
    assert!(cleared.items.is_empty());
    assert_eq!(cleared.revision, 2);
    handle.clear().unwrap();
    assert_eq!(
        handle
            .list(ClipboardEntriesQuery::default())
            .unwrap()
            .revision,
        2
    );
}

#[test]
fn image_artifact_and_database_insert_failures_leave_no_row_or_new_file() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();
    let images = workspace.data_dir().join("images");
    let thumbnails = workspace.data_dir().join("thumbnails");

    let artifact_retry_payload = vec![1, 2, 3, 255];
    std::fs::remove_dir(&images).unwrap();
    std::fs::write(&images, b"blocks image directory").unwrap();
    capture_image(handle, artifact_retry_payload.clone(), 1, 1);
    assert!(handle
        .list(ClipboardEntriesQuery::default())
        .unwrap()
        .items
        .is_empty());
    assert!(direct_children(&thumbnails).is_empty());

    std::fs::remove_file(&images).unwrap();
    std::fs::create_dir(&images).unwrap();
    capture_image(handle, artifact_retry_payload, 1, 1);
    let retried_artifact = wait_for_page(handle, "artifact failure retry", |page| {
        page.items.len() == 1
    });
    assert_eq!(
        retried_artifact.items[0].content_type,
        ClipboardContentType::Image
    );
    handle.clear().unwrap();

    let connection = open_encrypted_database(&workspace.database_path());
    connection
        .execute_batch(
            "CREATE TRIGGER reject_clipboard_insert
             BEFORE INSERT ON clipboard_entries
             BEGIN
                 SELECT RAISE(ABORT, 'injected insert failure');
             END;",
        )
        .unwrap();
    drop(connection);

    let database_retry_payload = vec![5, 6, 7, 255];
    capture_image(handle, database_retry_payload.clone(), 1, 1);
    assert!(handle
        .list(ClipboardEntriesQuery::default())
        .unwrap()
        .items
        .is_empty());
    assert!(direct_children(&images).is_empty());
    assert!(direct_children(&thumbnails).is_empty());

    let connection = open_encrypted_database(&workspace.database_path());
    connection
        .execute_batch("DROP TRIGGER reject_clipboard_insert;")
        .unwrap();
    drop(connection);
    capture_image(handle, database_retry_payload, 1, 1);
    let retried_database = wait_for_page(handle, "database failure retry", |page| {
        page.items.len() == 1
    });
    assert_eq!(
        retried_database.items[0].content_type,
        ClipboardContentType::Image
    );
}

#[test]
fn failed_text_insert_can_retry_the_same_payload_after_storage_recovers() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();
    let connection = open_encrypted_database(&workspace.database_path());
    connection
        .execute_batch(
            "CREATE TRIGGER reject_text_clipboard_insert
             BEFORE INSERT ON clipboard_entries
             BEGIN
                 SELECT RAISE(ABORT, 'injected text insert failure');
             END;",
        )
        .unwrap();
    drop(connection);

    capture_text(handle, "retry-after-storage-recovers");
    assert!(handle
        .list(ClipboardEntriesQuery::default())
        .unwrap()
        .items
        .is_empty());

    let connection = open_encrypted_database(&workspace.database_path());
    connection
        .execute_batch("DROP TRIGGER reject_text_clipboard_insert;")
        .unwrap();
    drop(connection);
    capture_text(handle, "retry-after-storage-recovers");
    let retried = wait_for_page(handle, "text database failure retry", |page| {
        page.items.len() == 1
    });
    assert_eq!(
        preview_text(&retried.items[0]),
        "retry-after-storage-recovers"
    );
}

#[test]
fn mailbox_orders_capture_clear_capture_without_a_second_writer() {
    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(20), false);
    let handle = running.handle();

    capture_text(handle, "before clear");
    handle.clear().unwrap();
    capture_text(handle, "after clear");
    let page = wait_for_page(handle, "capture after clear", |page| page.items.len() == 1);
    assert_eq!(preview_text(&page.items[0]), "after clear");
    assert_eq!(page.revision, 3);
}

#[test]
fn schema_v8_is_destructively_rebuilt_as_v9_and_resets_artifacts() {
    let workspace = TestWorkspace::new();
    std::fs::create_dir_all(workspace.data_dir().join("images")).unwrap();
    std::fs::create_dir_all(workspace.data_dir().join("thumbnails")).unwrap();
    std::fs::write(workspace.data_dir().join("images/legacy.png"), b"legacy").unwrap();
    std::fs::write(
        workspace.data_dir().join("thumbnails/legacy.jpg"),
        b"legacy",
    )
    .unwrap();
    let settings_path = workspace.data_dir().join("settings.db");
    std::fs::write(&settings_path, b"settings stay separate").unwrap();

    let connection = open_encrypted_database(&workspace.database_path());
    connection
        .execute_batch(
            "CREATE TABLE clipboard_entries (
                 id TEXT PRIMARY KEY,
                 status TEXT NOT NULL,
                 is_pinned INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE clipboard_jobs (id TEXT PRIMARY KEY);
             INSERT INTO clipboard_entries (id, status, is_pinned)
                 VALUES ('legacy-pinned', 'ready', 1);
             INSERT INTO clipboard_jobs (id) VALUES ('legacy-job');
             PRAGMA user_version = 8;",
        )
        .unwrap();
    drop(connection);

    let (running, _) = start_engine(&workspace, policy(20), false);
    assert!(running
        .handle()
        .list(ClipboardEntriesQuery::default())
        .unwrap()
        .items
        .is_empty());
    assert!(direct_children(&workspace.data_dir().join("images")).is_empty());
    assert!(direct_children(&workspace.data_dir().join("thumbnails")).is_empty());
    assert_eq!(
        std::fs::read(&settings_path).unwrap(),
        b"settings stay separate"
    );

    let connection = open_encrypted_database(&workspace.database_path());
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 9);
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .unwrap();
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(tables, ["clipboard_entries", "clipboard_entry_tags"]);
}

#[test]
fn normal_startup_removes_temp_and_orphan_files_but_keeps_referenced_images() {
    let workspace = TestWorkspace::new();
    let (first, _) = start_engine(&workspace, policy(20), false);
    capture_image(first.handle(), vec![11, 22, 33, 255], 1, 1);
    let stored = wait_for_page(first.handle(), "stored image", |page| page.items.len() == 1);
    let id = stored.items[0].id.clone();
    first.stop();

    let original = workspace
        .data_dir()
        .join("images")
        .join(format!("{id}.png"));
    let referenced_preview = direct_children(&workspace.data_dir().join("thumbnails"))
        .into_iter()
        .find(|path| path.file_stem().unwrap() == id.as_str())
        .unwrap();
    let orphan = workspace.data_dir().join("images/orphan.png");
    let temp = workspace.data_dir().join("thumbnails/interrupted.tmp");
    std::fs::write(&orphan, b"orphan").unwrap();
    std::fs::write(&temp, b"temp").unwrap();

    let (second, _) = start_engine(&workspace, policy(20), false);
    let page = second
        .handle()
        .list(ClipboardEntriesQuery::default())
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, id);
    assert!(original.is_file());
    assert!(referenced_preview.is_file());
    assert!(!orphan.exists());
    assert!(!temp.exists());
}

#[test]
fn fixed_seed_model_preserves_engine_invariants_for_ten_thousand_actions() {
    const ACTIONS: usize = 10_000;

    let workspace = TestWorkspace::new();
    let (running, _) = start_engine(&workspace, policy(8), false);
    let handle = running.handle();
    let mut seed = 0x5eed_0bad_cafe_f00d_u64;
    let mut max_history = 8_u32;
    let mut page = list_all_with_small_pages(handle);
    let mut action_counts = [0_usize; 5];

    for step in 0..ACTIONS {
        let prior_pinned = page
            .items
            .iter()
            .filter(|item| item.is_pinned)
            .map(|item| item.id.clone())
            .collect::<HashSet<_>>();
        let roll = next_seed(&mut seed) % 100;
        let preserves_pins = match roll {
            0..=54 => {
                capture_text(handle, format!("model-entry-{step}"));
                action_counts[0] += 1;
                true
            }
            55..=69 => {
                if !page.items.is_empty() {
                    let pinned_count = page.pinned_count as usize;
                    let candidates = if pinned_count >= 3 {
                        page.items
                            .iter()
                            .filter(|item| item.is_pinned)
                            .collect::<Vec<_>>()
                    } else {
                        page.items.iter().collect::<Vec<_>>()
                    };
                    let index = (next_seed(&mut seed) as usize) % candidates.len();
                    handle.toggle_pin(&candidates[index].id).unwrap();
                }
                action_counts[1] += 1;
                false
            }
            70..=82 => {
                if !page.items.is_empty() {
                    let index = (next_seed(&mut seed) as usize) % page.items.len();
                    handle.delete(&page.items[index].id).unwrap();
                }
                action_counts[2] += 1;
                false
            }
            83..=85 => {
                handle.clear().unwrap();
                action_counts[3] += 1;
                false
            }
            _ => {
                max_history = ((next_seed(&mut seed) % 10) + 1) as u32;
                handle.apply_policy(policy(max_history)).unwrap();
                action_counts[4] += 1;
                true
            }
        };

        page = list_all_with_small_pages(handle);
        assert_page_invariants(&page, max_history);
        if preserves_pins {
            let current_ids = page
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<HashSet<_>>();
            assert!(prior_pinned
                .iter()
                .all(|id| current_ids.contains(id.as_str())));
        }
    }

    assert_eq!(action_counts.iter().sum::<usize>(), ACTIONS);
    assert!(action_counts.into_iter().all(|count| count > 0));
}
