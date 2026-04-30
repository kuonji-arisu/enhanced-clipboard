use std::sync::{Arc, Mutex};

use chrono::Utc;
use log::{debug, warn};
use uuid::Uuid;

use crate::db::image_ingest_jobs::ImageIngestJobDraft;
use crate::models::{ClipboardEntry, EntryStatus};
use crate::services::artifacts::store;
use crate::services::image_ingest::{
    staging, CaptureImageDeps, MAX_ACTIVE_IMAGE_INGEST_JOBS, MAX_ACTIVE_IMAGE_STAGING_BYTES,
};
use crate::services::jobs::{clear_polling_image_dedup_if_current, ImageDedupState};
use crate::services::pipeline;
use crate::services::view_events::EventEmitter;

pub fn capture_image<A>(
    deps: CaptureImageDeps<'_, A>,
    img: &arboard::ImageData,
    source_app: String,
    image_dedup: Arc<Mutex<ImageDedupState>>,
    content_hash: String,
) -> Result<(), String>
where
    A: EventEmitter + Clone + Send + 'static,
{
    let id = Uuid::new_v4().to_string();
    let job_id = Uuid::new_v4().to_string();
    let width = img.width as u32;
    let height = img.height as u32;
    let byte_size = match staging::expected_rgba8_byte_size(width, height) {
        Ok(byte_size) => byte_size,
        Err(err) => {
            clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
            return Err(err);
        }
    };

    let backlog = match deps.db.image_ingest_backlog() {
        Ok(backlog) => backlog,
        Err(err) => {
            clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
            return Err(err);
        }
    };
    if backlog.count >= MAX_ACTIVE_IMAGE_INGEST_JOBS
        || backlog.byte_size.saturating_add(byte_size) > MAX_ACTIVE_IMAGE_STAGING_BYTES
    {
        clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
        return Err("Active image ingest backlog is full".to_string());
    }

    let entry = ClipboardEntry {
        id: id.clone(),
        content_type: "image".to_string(),
        status: EntryStatus::Pending,
        content: String::new(),
        canonical_search_text: String::new(),
        tags: Vec::new(),
        created_at: Utc::now().timestamp(),
        is_pinned: false,
        source_app,
    };
    debug!(
        "Queued image entry: id={}, width={}, height={}",
        entry.id, width, height
    );

    let input_ref = staging::input_rel_path(&job_id);
    if let Err(err) =
        staging::write_rgba8(deps.data_dir, &input_ref, img.bytes.as_ref(), width, height)
    {
        clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
        return Err(err);
    }

    let job = ImageIngestJobDraft {
        id: job_id,
        entry_id: id,
        input_ref: input_ref.clone(),
        dedup_key: content_hash.clone(),
        created_at: entry.created_at,
        width: i64::from(width),
        height: i64::from(height),
        pixel_format: staging::PIXEL_FORMAT_RGBA8.to_string(),
        byte_size,
        content_hash: content_hash.clone(),
    };

    if let Err(err) = deps.db.insert_pending_image_entry_with_job(
        &entry,
        &job,
        MAX_ACTIVE_IMAGE_INGEST_JOBS,
        MAX_ACTIVE_IMAGE_STAGING_BYTES,
    ) {
        store::cleanup_relative_paths(deps.data_dir, vec![input_ref]);
        clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
        return Err(err);
    }

    pipeline::emit_pending_entry_added(deps.app_handle, deps.db, deps.data_dir, &entry)?;
    if let Err(err) = deps.worker.wake() {
        warn!(
            "Image ingest job was queued but worker wake failed: {}",
            err
        );
    }

    Ok(())
}
