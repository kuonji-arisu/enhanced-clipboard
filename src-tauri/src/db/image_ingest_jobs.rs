use chrono::Utc;
use rusqlite::{params, types::Type, Connection, OptionalExtension};

use crate::db::clipboard::{row_to_entry, Database};
use crate::models::{
    ClipboardArtifactDraft, ClipboardEntry, ClipboardJob, ClipboardJobKind, ClipboardJobStatus,
    EntryStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageIngestJobDraft {
    pub id: String,
    pub entry_id: String,
    pub input_ref: String,
    pub dedup_key: String,
    pub created_at: i64,
    pub width: i64,
    pub height: i64,
    pub pixel_format: String,
    pub byte_size: i64,
    pub content_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageIngestBacklog {
    pub count: i64,
    pub byte_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageIngestJobCleanupRecord {
    pub entry_id: String,
    pub kind: ClipboardJobKind,
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
    pub active_jobs: Vec<ImageIngestJobCleanupRecord>,
}

impl Database {
    fn insert_image_ingest_job_on(
        conn: &Connection,
        job: &ImageIngestJobDraft,
    ) -> Result<(), String> {
        conn.execute(
            "INSERT INTO clipboard_jobs
             (id, entry_id, kind, status, input_ref, dedup_key, attempts, created_at, updated_at,
              width, height, pixel_format, byte_size, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                job.id,
                job.entry_id,
                ClipboardJobKind::ImageIngest.as_str(),
                ClipboardJobStatus::Queued.as_str(),
                job.input_ref,
                job.dedup_key,
                job.created_at,
                job.width,
                job.height,
                job.pixel_format,
                job.byte_size,
                job.content_hash,
            ],
        )
        .map_err(|e| format!("Failed to insert image ingest job: {}", e))?;
        Ok(())
    }

    fn image_ingest_backlog_on(conn: &Connection) -> Result<ImageIngestBacklog, String> {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(byte_size), 0)
             FROM clipboard_jobs
             WHERE kind = ?1 AND status IN (?2, ?3)",
            params![
                ClipboardJobKind::ImageIngest.as_str(),
                ClipboardJobStatus::Queued.as_str(),
                ClipboardJobStatus::Running.as_str(),
            ],
            |row| {
                Ok(ImageIngestBacklog {
                    count: row.get(0)?,
                    byte_size: row.get(1)?,
                })
            },
        )
        .map_err(|e| e.to_string())
    }

    fn active_job_cleanup_for_entries_on(
        conn: &Connection,
        ids: &[String],
    ) -> Result<Vec<ImageIngestJobCleanupRecord>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT entry_id, kind, input_ref, dedup_key
                 FROM clipboard_jobs
                 WHERE entry_id IN ({})
                   AND status IN ('queued', 'running')",
                placeholders
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
                let kind: String = row.get(1)?;
                Ok(ImageIngestJobCleanupRecord {
                    entry_id: row.get(0)?,
                    kind: job_kind_from_db(kind)?,
                    input_ref: row.get(2)?,
                    dedup_key: row.get(3)?,
                })
            })
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
            && backlog.byte_size.saturating_add(job.byte_size) > max_active_bytes
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
                   AND status IN (?3, ?4)
                 LIMIT 1",
                params![
                    ClipboardJobKind::ImageIngest.as_str(),
                    job.dedup_key,
                    ClipboardJobStatus::Queued.as_str(),
                    ClipboardJobStatus::Running.as_str(),
                ],
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

    pub fn get_job_by_id(&self, id: &str) -> Result<Option<ClipboardJob>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, entry_id, kind, status, input_ref, dedup_key, attempts,
                        created_at, updated_at, error, width, height, pixel_format,
                        byte_size, content_hash
                 FROM clipboard_jobs
                 WHERE id = ?1",
            )
            .map_err(|e| e.to_string())?;
        stmt.query_row(params![id], row_to_job)
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn get_active_image_ingest_jobs(&self) -> Result<Vec<ClipboardJob>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, entry_id, kind, status, input_ref, dedup_key, attempts,
                        created_at, updated_at, error, width, height, pixel_format,
                        byte_size, content_hash
                 FROM clipboard_jobs
                 WHERE kind = ?1 AND status IN (?2, ?3)
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![
                    ClipboardJobKind::ImageIngest.as_str(),
                    ClipboardJobStatus::Queued.as_str(),
                    ClipboardJobStatus::Running.as_str(),
                ],
                row_to_job,
            )
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
                 WHERE e.content_type = 'image'
                   AND e.status = ?1
                   AND NOT EXISTS (
                       SELECT 1
                       FROM clipboard_jobs j
                       WHERE j.entry_id = e.id
                         AND j.kind = ?2
                         AND j.status IN (?3, ?4)
                   )
                 ORDER BY e.created_at ASC, e.id ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![
                    EntryStatus::Pending.as_str(),
                    ClipboardJobKind::ImageIngest.as_str(),
                    ClipboardJobStatus::Queued.as_str(),
                    ClipboardJobStatus::Running.as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn requeue_running_image_ingest_jobs(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let updated = conn
            .execute(
                "UPDATE clipboard_jobs
                 SET status = ?1, updated_at = ?2, error = NULL
                 WHERE kind = ?3 AND status = ?4",
                params![
                    ClipboardJobStatus::Queued.as_str(),
                    Utc::now().timestamp(),
                    ClipboardJobKind::ImageIngest.as_str(),
                    ClipboardJobStatus::Running.as_str(),
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(updated)
    }

    pub fn cleanup_terminal_image_ingest_jobs(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM clipboard_jobs
             WHERE kind = ?1 AND status IN (?2, ?3, ?4)",
            params![
                ClipboardJobKind::ImageIngest.as_str(),
                ClipboardJobStatus::Succeeded.as_str(),
                ClipboardJobStatus::Failed.as_str(),
                ClipboardJobStatus::Canceled.as_str(),
            ],
        )
        .map_err(|e| e.to_string())
    }

    pub fn claim_next_image_ingest_job(&self) -> Result<Option<ClipboardJob>, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let job_id = tx
            .query_row(
                "SELECT id
                 FROM clipboard_jobs
                 WHERE kind = ?1 AND status = ?2
                 ORDER BY created_at ASC, id ASC
                 LIMIT 1",
                params![
                    ClipboardJobKind::ImageIngest.as_str(),
                    ClipboardJobStatus::Queued.as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(job_id) = job_id else {
            tx.rollback().map_err(|e| e.to_string())?;
            return Ok(None);
        };
        let now = Utc::now().timestamp();
        tx.execute(
            "UPDATE clipboard_jobs
             SET status = ?1, attempts = attempts + 1, updated_at = ?2, error = NULL
             WHERE id = ?3 AND status = ?4",
            params![
                ClipboardJobStatus::Running.as_str(),
                now,
                job_id,
                ClipboardJobStatus::Queued.as_str(),
            ],
        )
        .map_err(|e| e.to_string())?;
        let job = tx
            .query_row(
                "SELECT id, entry_id, kind, status, input_ref, dedup_key, attempts,
                        created_at, updated_at, error, width, height, pixel_format,
                        byte_size, content_hash
                 FROM clipboard_jobs
                 WHERE id = ?1",
                params![job_id],
                row_to_job,
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Some(job))
    }

    pub fn finalize_running_image_ingest_job(
        &self,
        job_id: &str,
        artifacts: &[ClipboardArtifactDraft],
    ) -> Result<JobFinalizeOutcome, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let job = tx
            .query_row(
                "SELECT id, entry_id, kind, status, input_ref, dedup_key, attempts,
                        created_at, updated_at, error, width, height, pixel_format,
                        byte_size, content_hash
                 FROM clipboard_jobs
                 WHERE id = ?1",
                params![job_id],
                row_to_job,
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(job) = job else {
            tx.rollback().map_err(|e| e.to_string())?;
            return Ok(JobFinalizeOutcome::Skipped);
        };
        if job.kind != ClipboardJobKind::ImageIngest || job.status != ClipboardJobStatus::Running {
            tx.rollback().map_err(|e| e.to_string())?;
            return Ok(JobFinalizeOutcome::Skipped);
        }

        let entry_status = tx
            .query_row(
                "SELECT status FROM clipboard_entries WHERE id = ?1 AND content_type = 'image'",
                params![job.entry_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if entry_status.as_deref() != Some(EntryStatus::Pending.as_str()) {
            tx.execute(
                "UPDATE clipboard_jobs SET status = ?1, updated_at = ?2, error = ?3 WHERE id = ?4",
                params![
                    ClipboardJobStatus::Canceled.as_str(),
                    Utc::now().timestamp(),
                    "pending entry disappeared before image ingest finalize",
                    job.id,
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(JobFinalizeOutcome::Skipped);
        }

        Self::insert_artifacts_on(&tx, &job.entry_id, artifacts)?;
        tx.execute(
            "UPDATE clipboard_entries SET status = ?1 WHERE id = ?2",
            params![EntryStatus::Ready.as_str(), job.entry_id],
        )
        .map_err(|e| format!("Failed to finalize pending entry: {}", e))?;
        tx.execute(
            "UPDATE clipboard_jobs SET status = ?1, updated_at = ?2, error = NULL WHERE id = ?3",
            params![
                ClipboardJobStatus::Succeeded.as_str(),
                Utc::now().timestamp(),
                job.id,
            ],
        )
        .map_err(|e| format!("Failed to mark image ingest job succeeded: {}", e))?;

        let entry = tx
            .query_row(
                "SELECT id, content_type, status, content, canonical_search_text, created_at, is_pinned, source_app
                 FROM clipboard_entries WHERE id = ?1",
                params![job.entry_id],
                row_to_entry,
            )
            .map_err(|e| format!("Failed to load finalized entry: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(JobFinalizeOutcome::Ready(entry))
    }

    pub fn requeue_running_image_ingest_job(
        &self,
        job_id: &str,
        error: &str,
    ) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let updated = conn
            .execute(
                "UPDATE clipboard_jobs
                 SET status = ?1, updated_at = ?2, error = ?3
                 WHERE id = ?4 AND status = ?5",
                params![
                    ClipboardJobStatus::Queued.as_str(),
                    Utc::now().timestamp(),
                    error,
                    job_id,
                    ClipboardJobStatus::Running.as_str(),
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(updated > 0)
    }

    pub fn fail_running_job_and_delete_pending_entry(
        &self,
        job_id: &str,
        error: &str,
    ) -> Result<Option<EntryJobCleanup>, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let job = tx
            .query_row(
                "SELECT id, entry_id, kind, status, input_ref, dedup_key, attempts,
                        created_at, updated_at, error, width, height, pixel_format,
                        byte_size, content_hash
                 FROM clipboard_jobs
                 WHERE id = ?1",
                params![job_id],
                row_to_job,
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(job) = job else {
            tx.rollback().map_err(|e| e.to_string())?;
            return Ok(None);
        };
        if job.status != ClipboardJobStatus::Running {
            tx.rollback().map_err(|e| e.to_string())?;
            return Ok(None);
        }

        tx.execute(
            "UPDATE clipboard_jobs SET status = ?1, updated_at = ?2, error = ?3 WHERE id = ?4",
            params![
                ClipboardJobStatus::Failed.as_str(),
                Utc::now().timestamp(),
                error,
                job.id,
            ],
        )
        .map_err(|e| e.to_string())?;

        let status = tx
            .query_row(
                "SELECT status FROM clipboard_entries WHERE id = ?1",
                params![job.entry_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if status.as_deref() != Some(EntryStatus::Pending.as_str()) {
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(None);
        }
        let ids = vec![job.entry_id.clone()];
        let artifact_paths = Self::artifact_paths_for_ids_on(&tx, &ids)?;
        let active_jobs = vec![ImageIngestJobCleanupRecord {
            entry_id: job.entry_id.clone(),
            kind: job.kind,
            input_ref: job.input_ref.clone(),
            dedup_key: job.dedup_key.clone(),
        }];
        tx.execute(
            "DELETE FROM clipboard_entries WHERE id = ?1",
            params![job.entry_id],
        )
        .map_err(|e| format!("Failed to delete failed pending entry: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Some(EntryJobCleanup {
            removed_ids: ids,
            artifact_paths,
            active_jobs,
        }))
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

    pub fn clear_all_with_job_cleanup(&self) -> Result<EntryJobCleanup, String> {
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

        let artifact_paths = Self::artifact_paths_for_ids_on(&tx, &ids)?;
        let active_jobs = Self::active_job_cleanup_for_entries_on(&tx, &ids)?;
        tx.execute("DELETE FROM clipboard_entries", [])
            .map_err(|e| format!("Failed to clear entries: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(EntryJobCleanup {
            removed_ids: ids,
            artifact_paths,
            active_jobs,
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

        let artifact_paths = Self::artifact_paths_for_ids_on(&tx, &rows)?;
        let active_jobs = Self::active_job_cleanup_for_entries_on(&tx, &rows)?;
        if !rows.is_empty() {
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

        Ok(EntryJobCleanup {
            removed_ids: rows,
            artifact_paths,
            active_jobs,
        })
    }
}

fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<ClipboardJob> {
    let kind: String = row.get(2)?;
    let status: String = row.get(3)?;
    Ok(ClipboardJob {
        id: row.get(0)?,
        entry_id: row.get(1)?,
        kind: job_kind_from_db(kind)?,
        status: job_status_from_db(status)?,
        input_ref: row.get(4)?,
        dedup_key: row.get(5)?,
        attempts: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        error: row.get(9)?,
        width: row.get(10)?,
        height: row.get(11)?,
        pixel_format: row.get(12)?,
        byte_size: row.get(13)?,
        content_hash: row.get(14)?,
    })
}

fn job_kind_from_db(kind: String) -> rusqlite::Result<ClipboardJobKind> {
    ClipboardJobKind::from_db(&kind).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        )
    })
}

fn job_status_from_db(status: String) -> rusqlite::Result<ClipboardJobStatus> {
    ClipboardJobStatus::from_db(&status).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        )
    })
}
