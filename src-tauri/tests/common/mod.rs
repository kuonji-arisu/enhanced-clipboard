#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use chrono::{Local, TimeZone};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tempfile::TempDir;

use enhanced_clipboard_lib::db::image_ingest_jobs::ImageIngestJobDraft;
use enhanced_clipboard_lib::db::{Database, SettingsStore};
use enhanced_clipboard_lib::i18n::I18n;
use enhanced_clipboard_lib::models::{
    ArtifactRole, ClipboardArtifact, ClipboardArtifactDraft, ClipboardEntry, ClipboardJobStatus,
    ClipboardPreview, EntryStatus,
};
use enhanced_clipboard_lib::services::image_ingest::{
    staging, MAX_ACTIVE_IMAGE_INGEST_JOBS, MAX_ACTIVE_IMAGE_STAGING_BYTES,
};
use enhanced_clipboard_lib::services::jobs::ImageDedupState;
use enhanced_clipboard_lib::services::persisted_state::PersistedApp;
use enhanced_clipboard_lib::services::search_preview::build_canonical_search_text;
use enhanced_clipboard_lib::services::settings::SettingsApp;
use enhanced_clipboard_lib::services::view_events::EventEmitter;

const TEST_DB_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn open_raw_clipboard_conn(ctx: &TestContext) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(ctx.data_dir.join("clipboard.db")).expect("open db");
    conn.execute_batch(&format!("PRAGMA key = \"x'{TEST_DB_KEY}'\";"))
        .expect("unlock db");
    conn
}

pub struct TestContext {
    pub _tempdir: TempDir,
    pub data_dir: PathBuf,
    pub db: Database,
    pub settings: SettingsStore,
    pub claims: Arc<Mutex<ImageDedupState>>,
}

impl TestContext {
    pub fn new() -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let data_dir = tempdir.path().join("data");
        std::fs::create_dir_all(data_dir.join("images")).expect("images dir");
        std::fs::create_dir_all(data_dir.join("thumbnails")).expect("thumbnails dir");
        std::fs::create_dir_all(data_dir.join("files")).expect("files dir");
        std::fs::create_dir_all(data_dir.join("previews")).expect("previews dir");

        let db = Database::new(
            data_dir.join("clipboard.db").to_string_lossy().as_ref(),
            TEST_DB_KEY,
            false,
        )
        .expect("clipboard db");
        let settings = SettingsStore::new(data_dir.join("settings.db").to_string_lossy().as_ref())
            .expect("settings db");

        Self {
            _tempdir: tempdir,
            data_dir,
            db,
            settings,
            claims: Arc::new(Mutex::new(ImageDedupState::default())),
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

    pub fn event_names(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("events")
            .iter()
            .map(|(name, _)| name.clone())
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
        if let Some(message) = self
            .autostart_error
            .lock()
            .expect("autostart_error")
            .clone()
        {
            Err(message)
        } else {
            Ok(())
        }
    }

    fn register_hotkey(&self, hotkey: &str) -> Result<(), String> {
        self.hotkey_calls
            .lock()
            .expect("hotkey_calls")
            .push(hotkey.to_string());
        if let Some(message) = self.hotkey_error.lock().expect("hotkey_error").clone() {
            Err(message)
        } else {
            Ok(())
        }
    }
}

impl PersistedApp for TestApp {
    fn set_always_on_top(&self, enabled: bool) -> Result<(), String> {
        self.always_on_top_calls
            .lock()
            .expect("always_on_top_calls")
            .push(enabled);
        if let Some(message) = self
            .always_on_top_error
            .lock()
            .expect("always_on_top_error")
            .clone()
        {
            Err(message)
        } else {
            Ok(())
        }
    }

    fn restore_window_position(&self, x: i32, y: i32) -> Result<(), String> {
        self.window_position_calls
            .lock()
            .expect("window_position_calls")
            .push((x, y));
        if let Some(message) = self
            .window_position_error
            .lock()
            .expect("window_position_error")
            .clone()
        {
            Err(message)
        } else {
            Ok(())
        }
    }
}

pub fn text_entry(id: &str, created_at: i64, content: &str) -> ClipboardEntry {
    ClipboardEntry {
        id: id.to_string(),
        content_type: "text".to_string(),
        status: EntryStatus::Ready,
        content: content.to_string(),
        canonical_search_text: build_canonical_search_text(content),
        tags: Vec::new(),
        created_at,
        is_pinned: false,
        source_app: "Code".to_string(),
    }
}

pub fn image_entry(id: &str, created_at: i64) -> ClipboardEntry {
    ClipboardEntry {
        id: id.to_string(),
        content_type: "image".to_string(),
        status: EntryStatus::Ready,
        content: String::new(),
        canonical_search_text: String::new(),
        tags: Vec::new(),
        created_at,
        is_pinned: false,
        source_app: "Photos".to_string(),
    }
}

pub fn pending_image_entry(id: &str, created_at: i64) -> ClipboardEntry {
    let mut entry = image_entry(id, created_at);
    entry.status = EntryStatus::Pending;
    entry
}

pub fn image_original_path(id: &str) -> String {
    format!("images/{id}.png")
}

pub fn image_display_path(id: &str) -> String {
    format!("thumbnails/{id}.png")
}

pub fn image_artifacts(id: &str) -> Vec<ClipboardArtifactDraft> {
    vec![
        ClipboardArtifactDraft {
            role: ArtifactRole::Original,
            rel_path: image_original_path(id),
            mime_type: "image/png".to_string(),
            width: Some(2),
            height: Some(2),
            byte_size: Some(4),
        },
        ClipboardArtifactDraft {
            role: ArtifactRole::Display,
            rel_path: image_display_path(id),
            mime_type: "image/png".to_string(),
            width: Some(2),
            height: Some(2),
            byte_size: Some(4),
        },
    ]
}

pub fn image_artifact_records(id: &str) -> Vec<ClipboardArtifact> {
    image_artifacts(id)
        .into_iter()
        .map(|artifact| ClipboardArtifact {
            entry_id: id.to_string(),
            role: artifact.role,
            rel_path: artifact.rel_path,
            mime_type: artifact.mime_type,
            width: artifact.width,
            height: artifact.height,
            byte_size: artifact.byte_size,
        })
        .collect()
}

pub fn pinned(mut entry: ClipboardEntry) -> ClipboardEntry {
    entry.is_pinned = true;
    entry
}

pub fn insert_entry(ctx: &TestContext, entry: &ClipboardEntry) {
    ctx.db.insert_entry(entry).expect("insert entry");
    if entry.content_type == "image" && entry.status == EntryStatus::Ready {
        ctx.db
            .insert_artifacts(&entry.id, &image_artifacts(&entry.id))
            .expect("insert image artifacts");
    }
}

pub fn insert_entry_with_tags(ctx: &TestContext, entry: &ClipboardEntry, tags: &[&str]) {
    let tag_values = tags.iter().map(|tag| tag.to_string()).collect::<Vec<_>>();
    ctx.db
        .insert_entry_with_attrs(entry, &[("tag", tag_values.as_slice())])
        .expect("insert entry with tags");
}

pub fn insert_pending_image_with_job(ctx: &TestContext, id: &str, created_at: i64) {
    let entry = pending_image_entry(id, created_at);
    let job = image_ingest_job_draft(ctx, id, created_at);
    ctx.db
        .insert_pending_image_entry_with_job(
            &entry,
            &job,
            MAX_ACTIVE_IMAGE_INGEST_JOBS,
            MAX_ACTIVE_IMAGE_STAGING_BYTES,
        )
        .expect("insert pending image job");
}

pub fn finalize_pending_image(ctx: &TestContext, id: &str) -> Option<ClipboardEntry> {
    let conn = open_raw_clipboard_conn(ctx);
    conn.execute(
        "UPDATE clipboard_jobs SET status = 'running' WHERE entry_id = ?1 AND kind = 'image_ingest'",
        [id],
    )
    .expect("mark image ingest job running");
    drop(conn);
    let job = ctx
        .db
        .get_active_image_ingest_jobs()
        .expect("active jobs")
        .into_iter()
        .find(|job| job.entry_id == id && job.status == ClipboardJobStatus::Running)
        .expect("running image ingest job");
    match ctx
        .db
        .finalize_running_image_ingest_job(&job.id, &image_artifacts(id))
        .expect("finalize pending image")
    {
        enhanced_clipboard_lib::db::JobFinalizeOutcome::Ready(entry) => Some(entry),
        enhanced_clipboard_lib::db::JobFinalizeOutcome::Skipped => None,
    }
}

fn image_ingest_job_draft(
    ctx: &TestContext,
    entry_id: &str,
    created_at: i64,
) -> ImageIngestJobDraft {
    let job_id = uuid::Uuid::new_v4().to_string();
    let input_ref = staging::input_rel_path(&job_id);
    let rgba = [255_u8, 255, 255, 255];
    let byte_size =
        staging::write_rgba8(&ctx.data_dir, &input_ref, &rgba, 1, 1).expect("write staging") as i64;
    ImageIngestJobDraft {
        id: job_id,
        entry_id: entry_id.to_string(),
        input_ref,
        dedup_key: format!("test-dedup-{entry_id}"),
        created_at,
        width: 1,
        height: 1,
        pixel_format: staging::PIXEL_FORMAT_RGBA8.to_string(),
        byte_size,
        content_hash: format!("test-hash-{entry_id}"),
    }
}

pub fn touch_file(ctx: &TestContext, relative_path: &str) {
    let full_path = ctx.data_dir.join(relative_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).expect("create asset parent");
    }
    std::fs::write(full_path, b"asset").expect("write asset");
}

pub fn local_date(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .expect("local datetime")
        .format("%Y-%m-%d")
        .to_string()
}

pub fn local_month(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .expect("local datetime")
        .format("%Y-%m")
        .to_string()
}

pub fn text_preview_text(preview: &ClipboardPreview) -> &str {
    match preview {
        ClipboardPreview::Text { text, .. } => text,
        ClipboardPreview::Image { .. } => panic!("expected text preview"),
    }
}
