use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::clipboard::{row_to_entry, Database};
use crate::models::{
    ClipboardArtifactDraft, ClipboardContentType, ClipboardEntry, ClipboardJob, ClipboardJobKind,
    EntryStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageIngestJobPayload {
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub byte_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageIngestJobDraft {
    pub entry_id: String,
    pub input_ref: String,
    pub dedup_key: String,
    pub created_at: i64,
    pub payload: ImageIngestJobPayload,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageIngestBacklog {
    pub count: i64,
    pub byte_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageIngestJobCleanupRecord {
    pub entry_id: String,
    pub input_ref: String,
    pub dedup_key: String,
}

#[derive(Debug, Clone)]
pub enum JobFinalizeOutcome {
    Ready(ClipboardEntry),
    Skipped,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryJobCleanup {
    pub removed_ids: Vec<String>,
    pub artifact_paths: Vec<String>,
    pub image_jobs: Vec<ImageIngestJobCleanupRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClearAllEntryIdsAndDedupKeys {
    pub removed_ids: Vec<String>,
    pub dedup_keys: Vec<String>,
}

impl ClipboardJob {
    pub fn image_ingest_payload(&self) -> Result<ImageIngestJobPayload, String> {
        serde_json::from_str(&self.payload_json)
            .map_err(|e| format!("Invalid image ingest job payload: {e}"))
    }
}

impl Database {
    pub(crate) fn entry_job_cleanup_on(
        conn: &Connection,
        removed_ids: Vec<String>,
    ) -> Result<EntryJobCleanup, String> {
        let artifact_paths = Self::artifact_paths_for_ids_on(conn, &removed_ids)?;
        let image_jobs = Self::image_job_cleanup_for_entries_on(conn, &removed_ids)?;
        Ok(EntryJobCleanup {
            removed_ids,
            artifact_paths,
            image_jobs,
        })
    }

    fn insert_image_ingest_job_on(
        conn: &Connection,
        job: &ImageIngestJobDraft,
    ) -> Result<(), String> {
        let payload_json = serde_json::to_string(&job.payload)
            .map_err(|e| format!("Failed to serialize image ingest job payload: {e}"))?;
        conn.execute(
            "INSERT INTO clipboard_jobs
             (entry_id, kind, created_at, input_ref, dedup_key, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                job.entry_id,
                ClipboardJobKind::ImageIngest.as_str(),
                job.created_at,
                job.input_ref,
                job.dedup_key,
                payload_json,
            ],
        )
        .map_err(|e| format!("Failed to insert image ingest job: {}", e))?;
        Ok(())
    }

    fn image_ingest_backlog_on(conn: &Connection) -> Result<ImageIngestBacklog, String> {
        let mut stmt = conn
            .prepare(
                "SELECT payload_json
                 FROM clipboard_jobs
                 WHERE kind = ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([ClipboardJobKind::ImageIngest.as_str()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;

        let mut backlog = ImageIngestBacklog::default();
        for row in rows {
            let payload_json = row.map_err(|e| e.to_string())?;
            let payload: ImageIngestJobPayload = serde_json::from_str(&payload_json)
                .map_err(|e| format!("Invalid image ingest job payload: {e}"))?;
            backlog.count += 1;
            backlog.byte_size = backlog.byte_size.saturating_add(payload.byte_size);
        }
        Ok(backlog)
    }

    fn image_job_cleanup_for_entries_on(
        conn: &Connection,
        ids: &[String],
    ) -> Result<Vec<ImageIngestJobCleanupRecord>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT entry_id, input_ref, dedup_key
                 FROM clipboard_jobs
                 WHERE kind = ?
                   AND entry_id IN ({})",
                placeholders
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(
                    std::iter::once(ClipboardJobKind::ImageIngest.as_str())
                        .chain(ids.iter().map(|id| id.as_str())),
                ),
                |row| {
                    Ok(ImageIngestJobCleanupRecord {
                        entry_id: row.get(0)?,
                        input_ref: row.get(1)?,
                        dedup_key: row.get(2)?,
                    })
                },
            )
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn image_ingest_backlog(&self) -> Result<ImageIngestBacklog, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        Self::image_ingest_backlog_on(&conn)
    }

    pub fn insert_pending_image_entry_with_job(
        &self,
        entry: &ClipboardEntry,
        job: &ImageIngestJobDraft,
        max_active_jobs: i64,
        max_active_bytes: i64,
    ) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let backlog = Self::image_ingest_backlog_on(&tx)?;
        if backlog.count >= max_active_jobs {
            tx.rollback().map_err(|e| e.to_string())?;
            return Err("Active image ingest backlog is full".to_string());
        }
        if max_active_bytes > 0
            && backlog.byte_size.saturating_add(job.payload.byte_size) > max_active_bytes
        {
            tx.rollback().map_err(|e| e.to_string())?;
            return Err("Active image ingest staging byte limit is full".to_string());
        }

        let duplicate_active = tx
            .query_row(
                "SELECT 1
                 FROM clipboard_jobs
                 WHERE kind = ?1
                   AND dedup_key = ?2
                 LIMIT 1",
                params![ClipboardJobKind::ImageIngest.as_str(), job.dedup_key],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if duplicate_active.is_some() {
            tx.rollback().map_err(|e| e.to_string())?;
            return Err("Active image ingest job already exists for this content".to_string());
        }

        Self::insert_entry_on(&tx, entry)?;
        Self::insert_image_ingest_job_on(&tx, job)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_job_by_entry(
        &self,
        entry_id: &str,
        kind: ClipboardJobKind,
    ) -> Result<Option<ClipboardJob>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT entry_id, kind, created_at, input_ref, dedup_key, payload_json
                 FROM clipboard_jobs
                 WHERE entry_id = ?1 AND kind = ?2",
            )
            .map_err(|e| e.to_string())?;
        stmt.query_row(params![entry_id, kind.as_str()], row_to_job)
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn get_active_image_ingest_jobs(&self) -> Result<Vec<ClipboardJob>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT entry_id, kind, created_at, input_ref, dedup_key, payload_json
                 FROM clipboard_jobs
                 WHERE kind = ?1
                 ORDER BY created_at ASC, entry_id ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([ClipboardJobKind::ImageIngest.as_str()], row_to_job)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn get_pending_image_entries_without_active_job(&self) -> Result<Vec<String>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT e.id
                 FROM clipboard_entries e
                 WHERE e.content_type = ?1
                   AND e.status = ?2
                   AND NOT EXISTS (
                       SELECT 1
                       FROM clipboard_jobs j
                       WHERE j.entry_id = e.id
                         AND j.kind = ?3
                   )
                 ORDER BY e.created_at ASC, e.id ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![
                    ClipboardContentType::Image.as_str(),
                    EntryStatus::Pending.as_str(),
                    ClipboardJobKind::ImageIngest.as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn delete_dangling_active_jobs(&self) -> Result<Vec<ImageIngestJobCleanupRecord>, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let mut stmt = tx
            .prepare(
                "SELECT j.entry_id, j.input_ref, j.dedup_key
                 FROM clipboard_jobs j
                 LEFT JOIN clipboard_entries e ON e.id = j.entry_id
                 WHERE j.kind = ?1 AND e.id IS NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([ClipboardJobKind::ImageIngest.as_str()], |row| {
                Ok(ImageIngestJobCleanupRecord {
                    entry_id: row.get(0)?,
                    input_ref: row.get(1)?,
                    dedup_key: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let cleanup = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);
        tx.execute(
            "DELETE FROM clipboard_jobs
             WHERE kind = ?1
               AND entry_id NOT IN (SELECT id FROM clipboard_entries)",
            [ClipboardJobKind::ImageIngest.as_str()],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(cleanup)
    }

    pub fn claim_next_image_ingest_job(&self) -> Result<Option<ClipboardJob>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT entry_id, kind, created_at, input_ref, dedup_key, payload_json
                 FROM clipboard_jobs
                 WHERE kind = ?1
                 ORDER BY created_at ASC, entry_id ASC
                 LIMIT 1",
            )
            .map_err(|e| e.to_string())?;
        stmt.query_row([ClipboardJobKind::ImageIngest.as_str()], row_to_job)
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn finalize_active_image_ingest_job(
        &self,
        entry_id: &str,
        artifacts: &[ClipboardArtifactDraft],
    ) -> Result<JobFinalizeOutcome, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let job_exists = tx
            .query_row(
                "SELECT 1 FROM clipboard_jobs WHERE entry_id = ?1 AND kind = ?2",
                params![entry_id, ClipboardJobKind::ImageIngest.as_str()],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .is_some();
        if !job_exists {
            tx.rollback().map_err(|e| e.to_string())?;
            return Ok(JobFinalizeOutcome::Skipped);
        }

        let entry_status = tx
            .query_row(
                "SELECT status FROM clipboard_entries WHERE id = ?1 AND content_type = ?2",
                params![entry_id, ClipboardContentType::Image.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if entry_status.as_deref() != Some(EntryStatus::Pending.as_str()) {
            tx.execute(
                "DELETE FROM clipboard_jobs WHERE entry_id = ?1 AND kind = ?2",
                params![entry_id, ClipboardJobKind::ImageIngest.as_str()],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(JobFinalizeOutcome::Skipped);
        }

        Self::insert_artifacts_on(&tx, entry_id, artifacts)?;
        tx.execute(
            "UPDATE clipboard_entries SET status = ?1 WHERE id = ?2",
            params![EntryStatus::Ready.as_str(), entry_id],
        )
        .map_err(|e| format!("Failed to finalize pending entry: {}", e))?;
        tx.execute(
            "DELETE FROM clipboard_jobs WHERE entry_id = ?1 AND kind = ?2",
            params![entry_id, ClipboardJobKind::ImageIngest.as_str()],
        )
        .map_err(|e| format!("Failed to remove finalized image ingest job: {}", e))?;

        let entry = tx
            .query_row(
                "SELECT id, content_type, status, content, canonical_search_text, created_at, is_pinned, source_app
                 FROM clipboard_entries WHERE id = ?1",
                params![entry_id],
                row_to_entry,
            )
            .map_err(|e| format!("Failed to load finalized entry: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(JobFinalizeOutcome::Ready(entry))
    }

    pub fn fail_active_image_ingest_job_and_delete_pending_entry(
        &self,
        entry_id: &str,
    ) -> Result<Option<EntryJobCleanup>, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let status = tx
            .query_row(
                "SELECT status FROM clipboard_entries WHERE id = ?1",
                params![entry_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if status.as_deref() != Some(EntryStatus::Pending.as_str()) {
            tx.execute(
                "DELETE FROM clipboard_jobs WHERE entry_id = ?1 AND kind = ?2",
                params![entry_id, ClipboardJobKind::ImageIngest.as_str()],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(None);
        }

        let cleanup = Self::entry_job_cleanup_on(&tx, vec![entry_id.to_string()])?;
        tx.execute(
            "DELETE FROM clipboard_entries WHERE id = ?1",
            params![entry_id],
        )
        .map_err(|e| format!("Failed to delete failed pending entry: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Some(cleanup))
    }

    pub fn delete_entry_with_job_cleanup(
        &self,
        id: &str,
    ) -> Result<Option<EntryJobCleanup>, String> {
        let ids = [id.to_string()];
        let cleanup = self.delete_entries_with_job_cleanup(&ids)?;
        if cleanup.removed_ids.is_empty() {
            Ok(None)
        } else {
            Ok(Some(cleanup))
        }
    }

    pub fn clear_all_entry_ids_and_image_dedup_keys(
        &self,
    ) -> Result<ClearAllEntryIdsAndDedupKeys, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        let mut stmt = tx
            .prepare("SELECT id FROM clipboard_entries")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let ids: Vec<String> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);

        let mut dedup_stmt = tx
            .prepare(
                "SELECT dedup_key
                 FROM clipboard_jobs
                 WHERE kind = ?1
                   AND dedup_key <> ''",
            )
            .map_err(|e| e.to_string())?;
        let dedup_rows = dedup_stmt
            .query_map([ClipboardJobKind::ImageIngest.as_str()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;
        let dedup_keys = dedup_rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(dedup_stmt);

        tx.execute("DELETE FROM clipboard_entries", [])
            .map_err(|e| format!("Failed to clear entries: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(ClearAllEntryIdsAndDedupKeys {
            removed_ids: ids,
            dedup_keys,
        })
    }

    pub fn delete_entries_with_job_cleanup(
        &self,
        ids: &[String],
    ) -> Result<EntryJobCleanup, String> {
        if ids.is_empty() {
            return Ok(EntryJobCleanup::default());
        }

        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut stmt = tx
            .prepare(&format!(
                "SELECT id
                 FROM clipboard_entries
                 WHERE id IN ({})",
                placeholders
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;
        let rows = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);

        let cleanup = Self::entry_job_cleanup_on(&tx, rows)?;
        if !cleanup.removed_ids.is_empty() {
            tx.execute(
                &format!(
                    "DELETE FROM clipboard_entries WHERE id IN ({})",
                    placeholders
                ),
                rusqlite::params_from_iter(ids.iter()),
            )
            .map_err(|e| format!("Failed to delete entries: {}", e))?;
        }
        tx.commit().map_err(|e| e.to_string())?;

        Ok(cleanup)
    }
}

fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<ClipboardJob> {
    let kind: String = row.get(1)?;
    Ok(ClipboardJob {
        entry_id: row.get(0)?,
        kind: job_kind_from_db(kind)?,
        created_at: row.get(2)?,
        input_ref: row.get(3)?,
        dedup_key: row.get(4)?,
        payload_json: row.get(5)?,
    })
}

fn job_kind_from_db(kind: String) -> rusqlite::Result<ClipboardJobKind> {
    ClipboardJobKind::from_db(&kind).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        )
    })
}
