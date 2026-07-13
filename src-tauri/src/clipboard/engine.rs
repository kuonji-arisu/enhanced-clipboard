use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;

use chrono::Utc;
use log::{debug, error, info, warn};
use uuid::Uuid;

use super::artifacts::{
    ImageArtifacts, ImageReadError, RebuildPreviewError, RebuiltPreview, WrittenImageArtifacts,
};
use super::read_model::{
    build_canonical_search_text, project_entry, CapturedPayload, ClipboardChanged,
    ClipboardContentType, ClipboardEntriesQuery, ClipboardListPage, ImagePreviewRepairOutcome,
};
use super::repository::{MutationResult, NewEntry, PinToggleResult, RecordPage, Repository};
use crate::constants::{
    DEFAULT_CAPTURE_IMAGES, DEFAULT_EXPIRY_SECONDS, DEFAULT_MAX_HISTORY, MAX_PINNED_ENTRIES,
};

const MAILBOX_CAPACITY: usize = 2;
const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_IMAGE_BYTES: usize = 104_857_600;

type EventSink = Arc<dyn Fn(ClipboardChanged) -> Result<(), String> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardPolicy {
    pub expiry_seconds: i64,
    pub max_history: u32,
    pub capture_images: bool,
}

impl Default for ClipboardPolicy {
    fn default() -> Self {
        Self {
            expiry_seconds: DEFAULT_EXPIRY_SECONDS,
            max_history: DEFAULT_MAX_HISTORY,
            capture_images: DEFAULT_CAPTURE_IMAGES,
        }
    }
}

#[derive(Debug)]
pub enum ClipboardError {
    MailboxClosed,
    EntryNotFound,
    PinLimitExceeded { limit: u32 },
    ImageOriginalUnavailable,
    InvalidPayload(String),
    Storage(String),
    Artifact(String),
    SystemClipboard(String),
    Initialization(String),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MailboxClosed => write!(formatter, "clipboard engine is unavailable"),
            Self::EntryNotFound => write!(formatter, "clipboard entry was not found"),
            Self::PinLimitExceeded { limit } => {
                write!(formatter, "clipboard pin limit reached ({limit})")
            }
            Self::ImageOriginalUnavailable => {
                write!(formatter, "clipboard image original is unavailable")
            }
            Self::InvalidPayload(message) => {
                write!(formatter, "invalid clipboard payload: {message}")
            }
            Self::Storage(message) => write!(formatter, "clipboard storage failed: {message}"),
            Self::Artifact(message) => write!(formatter, "clipboard artifact failed: {message}"),
            Self::SystemClipboard(message) => {
                write!(formatter, "system clipboard write failed: {message}")
            }
            Self::Initialization(message) => {
                write!(
                    formatter,
                    "clipboard engine initialization failed: {message}"
                )
            }
        }
    }
}

impl std::error::Error for ClipboardError {}

pub trait ClipboardWriter: Send {
    fn write_text(&mut self, text: &str) -> Result<(), String>;
    fn write_image(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct SystemClipboardWriter;

impl ClipboardWriter for SystemClipboardWriter {
    fn write_text(&mut self, text: &str) -> Result<(), String> {
        crate::utils::clipboard::write_text_to_clipboard(text)
    }

    fn write_image(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        clipboard
            .set_image(arboard::ImageData {
                width: width as usize,
                height: height as usize,
                bytes: Cow::Borrowed(rgba),
            })
            .map_err(|error| error.to_string())
    }
}

pub struct ClipboardEngineConfig {
    database_path: PathBuf,
    database_key: String,
    recreate_database: bool,
    data_dir: PathBuf,
    initial_policy: ClipboardPolicy,
    event_sink: EventSink,
    clipboard_writer: Box<dyn ClipboardWriter>,
}

impl ClipboardEngineConfig {
    pub fn new(
        database_path: impl Into<PathBuf>,
        database_key: impl Into<String>,
        recreate_database: bool,
        data_dir: impl Into<PathBuf>,
        initial_policy: ClipboardPolicy,
        event_sink: impl Fn(ClipboardChanged) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            database_path: database_path.into(),
            database_key: database_key.into(),
            recreate_database,
            data_dir: data_dir.into(),
            initial_policy,
            event_sink: Arc::new(event_sink),
            clipboard_writer: Box::new(SystemClipboardWriter),
        }
    }

    pub fn with_clipboard_writer(mut self, writer: impl ClipboardWriter + 'static) -> Self {
        self.clipboard_writer = Box::new(writer);
        self
    }
}

pub struct ClipboardEngine {
    handle: ClipboardEngineHandle,
}

impl ClipboardEngine {
    pub fn start(config: ClipboardEngineConfig) -> Result<Self, ClipboardError> {
        let (request_tx, request_rx) = mpsc::sync_channel(MAILBOX_CAPACITY);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);

        thread::Builder::new()
            .name("clipboard-engine".to_string())
            .spawn(move || run_engine(config, request_rx, ready_tx))
            .map_err(|error| ClipboardError::Initialization(error.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                handle: ClipboardEngineHandle { sender: request_tx },
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(ClipboardError::Initialization(
                "engine thread exited before initialization completed".to_string(),
            )),
        }
    }

    pub fn handle(&self) -> ClipboardEngineHandle {
        self.handle.clone()
    }

    pub fn into_handle(self) -> ClipboardEngineHandle {
        self.handle
    }
}

#[derive(Clone)]
pub struct ClipboardEngineHandle {
    sender: SyncSender<ClipboardRequest>,
}

impl ClipboardEngineHandle {
    pub fn prime(&self, payload: CapturedPayload) -> Result<(), TrySendError<CapturedPayload>> {
        self.try_send_payload(payload, true)
    }

    pub fn try_capture(
        &self,
        payload: CapturedPayload,
    ) -> Result<(), TrySendError<CapturedPayload>> {
        self.try_send_payload(payload, false)
    }

    pub fn try_observe_no_capture(&self) -> Result<(), TrySendError<()>> {
        self.sender
            .try_send(ClipboardRequest::ObservedNoCapture)
            .map_err(|error| match error {
                TrySendError::Full(_) => TrySendError::Full(()),
                TrySendError::Disconnected(_) => TrySendError::Disconnected(()),
            })
    }

    pub fn list(&self, query: ClipboardEntriesQuery) -> Result<ClipboardListPage, ClipboardError> {
        self.request(|reply| ClipboardRequest::List { query, reply })
    }

    pub fn copy(&self, id: impl Into<String>) -> Result<(), ClipboardError> {
        self.request(|reply| ClipboardRequest::Copy {
            id: id.into(),
            reply,
        })
    }

    pub fn delete(&self, id: impl Into<String>) -> Result<(), ClipboardError> {
        self.request(|reply| ClipboardRequest::Delete {
            id: id.into(),
            reply,
        })
    }

    pub fn toggle_pin(&self, id: impl Into<String>) -> Result<(), ClipboardError> {
        self.request(|reply| ClipboardRequest::TogglePin {
            id: id.into(),
            reply,
        })
    }

    pub fn clear(&self) -> Result<(), ClipboardError> {
        self.request(|reply| ClipboardRequest::Clear { reply })
    }

    pub fn repair_preview(
        &self,
        id: impl Into<String>,
    ) -> Result<ImagePreviewRepairOutcome, ClipboardError> {
        self.request(|reply| ClipboardRequest::RepairPreview {
            id: id.into(),
            reply,
        })
    }

    pub fn get_active_dates(
        &self,
        year_month: impl Into<String>,
    ) -> Result<Vec<String>, ClipboardError> {
        self.request(|reply| ClipboardRequest::GetActiveDates {
            year_month: year_month.into(),
            reply,
        })
    }

    pub fn get_earliest_month(&self) -> Result<Option<String>, ClipboardError> {
        self.request(|reply| ClipboardRequest::GetEarliestMonth { reply })
    }

    pub fn apply_policy(&self, policy: ClipboardPolicy) -> Result<(), ClipboardError> {
        self.request(|reply| ClipboardRequest::ApplyPolicy { policy, reply })
    }

    fn try_send_payload(
        &self,
        payload: CapturedPayload,
        prime: bool,
    ) -> Result<(), TrySendError<CapturedPayload>> {
        let request = if prime {
            ClipboardRequest::Prime(payload)
        } else {
            ClipboardRequest::Capture(payload)
        };
        self.sender.try_send(request).map_err(|error| match error {
            TrySendError::Full(request) => TrySendError::Full(request.into_payload()),
            TrySendError::Disconnected(request) => {
                TrySendError::Disconnected(request.into_payload())
            }
        })
    }

    fn request<T>(
        &self,
        build: impl FnOnce(Sender<Result<T, ClipboardError>>) -> ClipboardRequest,
    ) -> Result<T, ClipboardError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(build(reply_tx))
            .map_err(|_| ClipboardError::MailboxClosed)?;
        reply_rx.recv().map_err(|_| ClipboardError::MailboxClosed)?
    }
}

enum ClipboardRequest {
    Prime(CapturedPayload),
    Capture(CapturedPayload),
    ObservedNoCapture,
    List {
        query: ClipboardEntriesQuery,
        reply: Sender<Result<ClipboardListPage, ClipboardError>>,
    },
    Copy {
        id: String,
        reply: Sender<Result<(), ClipboardError>>,
    },
    Delete {
        id: String,
        reply: Sender<Result<(), ClipboardError>>,
    },
    TogglePin {
        id: String,
        reply: Sender<Result<(), ClipboardError>>,
    },
    Clear {
        reply: Sender<Result<(), ClipboardError>>,
    },
    RepairPreview {
        id: String,
        reply: Sender<Result<ImagePreviewRepairOutcome, ClipboardError>>,
    },
    GetActiveDates {
        year_month: String,
        reply: Sender<Result<Vec<String>, ClipboardError>>,
    },
    GetEarliestMonth {
        reply: Sender<Result<Option<String>, ClipboardError>>,
    },
    ApplyPolicy {
        policy: ClipboardPolicy,
        reply: Sender<Result<(), ClipboardError>>,
    },
}

impl ClipboardRequest {
    fn into_payload(self) -> CapturedPayload {
        match self {
            Self::Prime(payload) | Self::Capture(payload) => payload,
            _ => unreachable!("payload mailbox request changed before try_send returned"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClipboardFingerprint {
    Text(String),
    Image(String),
}

struct EngineState {
    repository: Repository,
    artifacts: ImageArtifacts,
    policy: ClipboardPolicy,
    revision: u64,
    last_observed_fingerprint: Option<ClipboardFingerprint>,
    event_sink: EventSink,
    clipboard_writer: Box<dyn ClipboardWriter>,
}

impl EngineState {
    fn initialize(config: ClipboardEngineConfig) -> Result<Self, ClipboardError> {
        let mut repository = Repository::open(
            &config.database_path,
            &config.database_key,
            config.recreate_database,
        )
        .map_err(ClipboardError::Initialization)?;
        let artifacts = ImageArtifacts::new(config.data_dir);

        if repository.schema_rebuilt() {
            artifacts
                .reset_roots()
                .map_err(ClipboardError::Initialization)?;
        } else {
            artifacts
                .ensure_dirs()
                .map_err(ClipboardError::Initialization)?;
        }

        let now = Utc::now().timestamp();
        let startup_prune = repository
            .prune(
                window_start(now, config.initial_policy.expiry_seconds),
                config.initial_policy.max_history,
            )
            .map_err(ClipboardError::Initialization)?;
        artifacts.cleanup_paths(&startup_prune.cleanup_paths);

        let referenced_paths = repository
            .referenced_paths()
            .map_err(ClipboardError::Initialization)?;
        let cleanup_report = artifacts
            .cleanup_startup(&referenced_paths)
            .map_err(ClipboardError::Initialization)?;
        if cleanup_report.removed_files > 0 || cleanup_report.failed_removals > 0 {
            info!(
                "Clipboard startup artifact cleanup completed: removed={}, failed={}",
                cleanup_report.removed_files, cleanup_report.failed_removals
            );
        }

        Ok(Self {
            repository,
            artifacts,
            policy: config.initial_policy,
            revision: 0,
            last_observed_fingerprint: None,
            event_sink: config.event_sink,
            clipboard_writer: config.clipboard_writer,
        })
    }

    fn handle(&mut self, request: ClipboardRequest) {
        match request {
            ClipboardRequest::Prime(payload) => self.prime(payload),
            ClipboardRequest::Capture(payload) => {
                if let Err(error) = self.capture(payload) {
                    error!("Failed to process clipboard capture: {error}");
                }
            }
            ClipboardRequest::ObservedNoCapture => {
                self.last_observed_fingerprint = None;
            }
            ClipboardRequest::List { query, reply } => {
                let _ = reply.send(self.list(query));
            }
            ClipboardRequest::Copy { id, reply } => {
                let _ = reply.send(self.copy(&id));
            }
            ClipboardRequest::Delete { id, reply } => {
                let _ = reply.send(self.delete(&id));
            }
            ClipboardRequest::TogglePin { id, reply } => {
                let _ = reply.send(self.toggle_pin(&id));
            }
            ClipboardRequest::Clear { reply } => {
                let _ = reply.send(self.clear());
            }
            ClipboardRequest::RepairPreview { id, reply } => {
                let _ = reply.send(self.repair_preview(&id));
            }
            ClipboardRequest::GetActiveDates { year_month, reply } => {
                let _ = reply.send(self.active_dates(&year_month));
            }
            ClipboardRequest::GetEarliestMonth { reply } => {
                let _ = reply.send(self.earliest_month());
            }
            ClipboardRequest::ApplyPolicy { policy, reply } => {
                let _ = reply.send(self.apply_policy(policy));
            }
        }
    }

    fn prime(&mut self, payload: CapturedPayload) {
        match payload {
            CapturedPayload::Text { content, .. } => {
                self.last_observed_fingerprint =
                    Some(ClipboardFingerprint::Text(hash_text(&content)));
            }
            CapturedPayload::Image {
                rgba,
                width,
                height,
                ..
            } => {
                if validate_image_payload(&rgba, width, height).is_err()
                    || !self.policy.capture_images
                    || rgba.len() > MAX_IMAGE_BYTES
                {
                    self.last_observed_fingerprint = None;
                    return;
                }
                self.last_observed_fingerprint = Some(ClipboardFingerprint::Image(hash_image(
                    &rgba, width, height,
                )));
            }
        }
    }

    fn capture(&mut self, payload: CapturedPayload) -> Result<(), ClipboardError> {
        match payload {
            CapturedPayload::Text {
                content,
                source_app,
            } => self.capture_text(content, source_app),
            CapturedPayload::Image {
                rgba,
                width,
                height,
                source_app,
            } => self.capture_image(rgba, width, height, source_app),
        }
    }

    fn capture_text(&mut self, content: String, source_app: String) -> Result<(), ClipboardError> {
        if content.is_empty() {
            return Ok(());
        }

        let fingerprint = ClipboardFingerprint::Text(hash_text(&content));
        if self.last_observed_fingerprint.as_ref() == Some(&fingerprint) {
            debug!("Ignored duplicate text clipboard capture");
            return Ok(());
        }
        self.last_observed_fingerprint = Some(fingerprint);

        if content.len() > MAX_TEXT_BYTES {
            debug!(
                "Ignored oversized text clipboard capture: bytes={}, max_bytes={}",
                content.len(),
                MAX_TEXT_BYTES
            );
            return Ok(());
        }

        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        let canonical_search_text = build_canonical_search_text(&content);
        let tags = detect_text_tags(&content);
        let insert = self.repository.insert_entry(
            NewEntry {
                id: id.clone(),
                content_type: ClipboardContentType::Text,
                content,
                canonical_search_text,
                created_at: now,
                source_app,
                original_rel_path: None,
                preview_rel_path: None,
                tags,
            },
            window_start(now, self.policy.expiry_seconds),
            self.policy.max_history,
        );
        let result = match insert {
            Ok(result) => result,
            Err(error) => {
                // A failed capture breaks the observed sequence, but must not
                // suppress a later retry of the same clipboard payload.
                self.last_observed_fingerprint = None;
                return Err(ClipboardError::Storage(error));
            }
        };
        self.finish_mutation(result);
        debug!("Stored text clipboard entry: id={id}");
        Ok(())
    }

    fn capture_image(
        &mut self,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        source_app: String,
    ) -> Result<(), ClipboardError> {
        validate_image_payload(&rgba, width, height)?;

        if !self.policy.capture_images {
            self.last_observed_fingerprint = None;
            return Ok(());
        }

        if rgba.len() > MAX_IMAGE_BYTES {
            self.last_observed_fingerprint = None;
            debug!(
                "Ignored oversized image clipboard capture: bytes={}, max_bytes={}",
                rgba.len(),
                MAX_IMAGE_BYTES
            );
            return Ok(());
        }

        let fingerprint = ClipboardFingerprint::Image(hash_image(&rgba, width, height));
        if self.last_observed_fingerprint.as_ref() == Some(&fingerprint) {
            debug!("Ignored duplicate image clipboard capture");
            return Ok(());
        }
        self.last_observed_fingerprint = Some(fingerprint);

        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        let written = match self.artifacts.write_image(&id, &rgba, width, height) {
            Ok(written) => written,
            Err(error) => {
                self.last_observed_fingerprint = None;
                return Err(ClipboardError::Artifact(error));
            }
        };
        let new_paths = vec![
            written.original_rel_path.clone(),
            written.preview_rel_path.clone(),
        ];
        let insert = self.repository.insert_entry(
            image_entry(&id, now, source_app, written),
            window_start(now, self.policy.expiry_seconds),
            self.policy.max_history,
        );
        let result = match insert {
            Ok(result) => result,
            Err(error) => {
                self.artifacts.cleanup_paths(&new_paths);
                self.last_observed_fingerprint = None;
                return Err(ClipboardError::Storage(error));
            }
        };
        self.finish_mutation(result);
        debug!("Stored image clipboard entry: id={id}, width={width}, height={height}");
        Ok(())
    }

    fn list(&self, query: ClipboardEntriesQuery) -> Result<ClipboardListPage, ClipboardError> {
        let now = Utc::now().timestamp();
        let query_text = query.text();
        let RecordPage {
            pinned,
            normal,
            next_cursor,
            pinned_count,
        } = self
            .repository
            .list_records(&query, window_start(now, self.policy.expiry_seconds))
            .map_err(ClipboardError::Storage)?;
        let items = pinned
            .into_iter()
            .chain(normal)
            .map(|entry| {
                project_entry(
                    entry,
                    self.artifacts.data_dir(),
                    query_text,
                    self.policy.expiry_seconds,
                )
            })
            .collect();
        Ok(ClipboardListPage {
            revision: self.revision,
            items,
            next_cursor,
            pinned_count,
        })
    }

    fn copy(&mut self, id: &str) -> Result<(), ClipboardError> {
        let entry = self
            .repository
            .get_entry(id)
            .map_err(ClipboardError::Storage)?
            .ok_or(ClipboardError::EntryNotFound)?;

        match entry.content_type {
            ClipboardContentType::Text => {
                self.clipboard_writer
                    .write_text(&entry.content)
                    .map_err(ClipboardError::SystemClipboard)?;
                self.last_observed_fingerprint =
                    Some(ClipboardFingerprint::Text(hash_text(&entry.content)));
                Ok(())
            }
            ClipboardContentType::Image => {
                let Some(original_rel_path) = entry.original_rel_path else {
                    self.remove_broken_image(id)?;
                    return Err(ClipboardError::ImageOriginalUnavailable);
                };
                let decoded = match self.artifacts.read_original_rgba(&original_rel_path) {
                    Ok(decoded) => decoded,
                    Err(ImageReadError::Missing | ImageReadError::Broken(_)) => {
                        self.remove_broken_image(id)?;
                        return Err(ClipboardError::ImageOriginalUnavailable);
                    }
                };
                self.clipboard_writer
                    .write_image(&decoded.rgba, decoded.width, decoded.height)
                    .map_err(ClipboardError::SystemClipboard)?;
                self.last_observed_fingerprint = Some(ClipboardFingerprint::Image(hash_image(
                    &decoded.rgba,
                    decoded.width,
                    decoded.height,
                )));
                Ok(())
            }
        }
    }

    fn delete(&mut self, id: &str) -> Result<(), ClipboardError> {
        let result = self
            .repository
            .delete_entry(id)
            .map_err(ClipboardError::Storage)?;
        self.finish_mutation(result);
        Ok(())
    }

    fn toggle_pin(&mut self, id: &str) -> Result<(), ClipboardError> {
        let now = Utc::now().timestamp();
        let result = self
            .repository
            .toggle_pin(
                id,
                MAX_PINNED_ENTRIES,
                window_start(now, self.policy.expiry_seconds),
                self.policy.max_history,
            )
            .map_err(ClipboardError::Storage)?;
        match result {
            PinToggleResult::Updated {
                is_pinned,
                mutation,
            } => {
                debug!("Updated clipboard pin state: id={id}, is_pinned={is_pinned}");
                self.finish_mutation(mutation);
                Ok(())
            }
            PinToggleResult::NotFound => Err(ClipboardError::EntryNotFound),
            PinToggleResult::LimitExceeded => Err(ClipboardError::PinLimitExceeded {
                limit: MAX_PINNED_ENTRIES,
            }),
        }
    }

    fn clear(&mut self) -> Result<(), ClipboardError> {
        let result = self.repository.clear().map_err(ClipboardError::Storage)?;
        self.finish_mutation(result);
        Ok(())
    }

    fn repair_preview(&mut self, id: &str) -> Result<ImagePreviewRepairOutcome, ClipboardError> {
        let entry = self
            .repository
            .get_entry(id)
            .map_err(ClipboardError::Storage)?
            .ok_or(ClipboardError::EntryNotFound)?;
        if entry.content_type != ClipboardContentType::Image {
            return Ok(ImagePreviewRepairOutcome::Unchanged);
        }
        let Some(original_rel_path) = entry.original_rel_path else {
            self.remove_broken_image(id)?;
            return Ok(ImagePreviewRepairOutcome::Removed);
        };

        let rebuilt = match self.artifacts.rebuild_preview(id, &original_rel_path) {
            Ok(rebuilt) => rebuilt,
            Err(RebuildPreviewError::OriginalMissing | RebuildPreviewError::OriginalBroken(_)) => {
                self.remove_broken_image(id)?;
                return Ok(ImagePreviewRepairOutcome::Removed);
            }
            Err(RebuildPreviewError::PreviewWrite(error)) => {
                return Err(ClipboardError::Artifact(error));
            }
        };
        self.commit_rebuilt_preview(id, rebuilt)?;
        Ok(ImagePreviewRepairOutcome::Repaired)
    }

    fn commit_rebuilt_preview(
        &mut self,
        id: &str,
        rebuilt: RebuiltPreview,
    ) -> Result<(), ClipboardError> {
        let replace = self
            .repository
            .replace_preview_path(id, &rebuilt.preview_rel_path);
        let replace = match replace {
            Ok(Some(replace)) => replace,
            Ok(None) => {
                self.artifacts
                    .cleanup_paths(std::slice::from_ref(&rebuilt.preview_rel_path));
                return Err(ClipboardError::EntryNotFound);
            }
            Err(error) => {
                self.artifacts
                    .cleanup_paths(std::slice::from_ref(&rebuilt.preview_rel_path));
                return Err(ClipboardError::Storage(error));
            }
        };

        let mut cleanup_paths = rebuilt.obsolete_preview_rel_paths;
        if let Some(old_path) = replace.old_path {
            if old_path != rebuilt.preview_rel_path {
                cleanup_paths.push(old_path);
            }
        }
        self.artifacts.cleanup_paths(&cleanup_paths);
        // The repository now points at the freshly committed preview path and
        // the superseded file has been cleaned up best-effort.
        self.notify_changed();
        Ok(())
    }

    fn active_dates(&self, year_month: &str) -> Result<Vec<String>, ClipboardError> {
        let now = Utc::now().timestamp();
        self.repository
            .active_dates(year_month, window_start(now, self.policy.expiry_seconds))
            .map_err(ClipboardError::Storage)
    }

    fn earliest_month(&self) -> Result<Option<String>, ClipboardError> {
        let now = Utc::now().timestamp();
        self.repository
            .earliest_month(window_start(now, self.policy.expiry_seconds))
            .map_err(ClipboardError::Storage)
    }

    fn apply_policy(&mut self, policy: ClipboardPolicy) -> Result<(), ClipboardError> {
        let expiry_changed = self.policy.expiry_seconds != policy.expiry_seconds;
        self.policy = policy;
        let now = Utc::now().timestamp();
        match self
            .repository
            .prune(window_start(now, policy.expiry_seconds), policy.max_history)
        {
            Ok(result) => {
                self.artifacts.cleanup_paths(&result.cleanup_paths);
                if result.changed || expiry_changed {
                    self.notify_changed();
                }
                Ok(())
            }
            Err(error) => {
                // Settings are already saved and the in-memory policy remains
                // authoritative. Publish a changed TTL projection now; a
                // later insert or restart retries retention cleanup.
                if expiry_changed {
                    self.notify_changed();
                }
                Err(ClipboardError::Storage(error))
            }
        }
    }

    fn remove_broken_image(&mut self, id: &str) -> Result<(), ClipboardError> {
        let result = self
            .repository
            .delete_entry(id)
            .map_err(ClipboardError::Storage)?;
        self.finish_mutation(result);
        Ok(())
    }

    fn finish_mutation(&mut self, result: MutationResult) {
        self.artifacts.cleanup_paths(&result.cleanup_paths);
        if result.changed {
            self.notify_changed();
        }
    }

    fn notify_changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
        let payload = ClipboardChanged {
            revision: self.revision,
        };
        if let Err(error) = (self.event_sink)(payload) {
            warn!(
                "Failed to emit clipboard invalidation at revision {}: {}",
                self.revision, error
            );
        }
    }
}

fn run_engine(
    config: ClipboardEngineConfig,
    requests: Receiver<ClipboardRequest>,
    ready: SyncSender<Result<(), ClipboardError>>,
) {
    let mut state = match EngineState::initialize(config) {
        Ok(state) => state,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    info!("Clipboard engine started");
    while let Ok(request) = requests.recv() {
        state.handle(request);
    }
    info!("Clipboard engine stopped");
}

fn image_entry(
    id: &str,
    created_at: i64,
    source_app: String,
    written: WrittenImageArtifacts,
) -> NewEntry {
    NewEntry {
        id: id.to_string(),
        content_type: ClipboardContentType::Image,
        content: String::new(),
        canonical_search_text: String::new(),
        created_at,
        source_app,
        original_rel_path: Some(written.original_rel_path),
        preview_rel_path: Some(written.preview_rel_path),
        tags: Vec::new(),
    }
}

fn window_start(now: i64, expiry_seconds: i64) -> Option<i64> {
    (expiry_seconds > 0).then(|| now.saturating_sub(expiry_seconds))
}

fn validate_image_payload(rgba: &[u8], width: u32, height: u32) -> Result<(), ClipboardError> {
    if width == 0 || height == 0 {
        return Err(ClipboardError::InvalidPayload(
            "image dimensions must be non-zero".to_string(),
        ));
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ClipboardError::InvalidPayload("image dimensions overflow".to_string()))?;
    if rgba.len() != expected {
        return Err(ClipboardError::InvalidPayload(format!(
            "RGBA byte length is {}, expected {expected}",
            rgba.len()
        )));
    }
    Ok(())
}

fn hash_text(text: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(text.len() as u64).to_le_bytes());
    hasher.update(text.as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn hash_image(rgba: &[u8], width: u32, height: u32) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&width.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    hasher.update(&(rgba.len() as u64).to_le_bytes());
    hasher.update(rgba);
    hasher.finalize().to_hex().to_string()
}

fn detect_text_tags(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if ((trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']')))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return vec!["json".to_string()];
    }
    if is_single_token(trimmed)
        && (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        && trimmed.len() > "https://".len()
    {
        return vec!["url".to_string()];
    }
    if is_single_token(trimmed) && looks_like_email(trimmed) {
        return vec!["email".to_string()];
    }
    Vec::new()
}

fn is_single_token(value: &str) -> bool {
    !value.is_empty() && value.split_whitespace().nth(1).is_none()
}

fn looks_like_email(value: &str) -> bool {
    if value.contains("://") {
        return false;
    }
    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && local
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-'))
        && domain
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
        && domain.split('.').all(|segment| {
            !segment.is_empty() && !segment.starts_with('-') && !segment.ends_with('-')
        })
}
