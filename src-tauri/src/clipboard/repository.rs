use std::collections::{HashMap, HashSet};
use std::path::Path;

use log::warn;
use rusqlite::{
    params,
    types::{Type, Value},
    Connection, OptionalExtension,
};

use super::read_model::{
    canonicalize_query_text, ClipboardContentType, ClipboardEntriesQuery, ClipboardQueryCursor,
    EntryRecord,
};

const SCHEMA_VERSION: i64 = 9;
const SQLITE_PARAM_BATCH: usize = 400;

#[derive(Debug, Clone)]
pub(crate) struct NewEntry {
    pub(crate) id: String,
    pub(crate) content_type: ClipboardContentType,
    pub(crate) content: String,
    pub(crate) canonical_search_text: String,
    pub(crate) created_at: i64,
    pub(crate) source_app: String,
    pub(crate) original_rel_path: Option<String>,
    pub(crate) preview_rel_path: Option<String>,
    pub(crate) tags: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MutationResult {
    pub(crate) changed: bool,
    pub(crate) cleanup_paths: Vec<String>,
}

impl MutationResult {
    fn inserted(mut self) -> Self {
        self.changed = true;
        self
    }

    fn merge(&mut self, other: Self) {
        self.changed |= other.changed;

        let mut seen = self.cleanup_paths.iter().cloned().collect::<HashSet<_>>();
        self.cleanup_paths.extend(
            other
                .cleanup_paths
                .into_iter()
                .filter(|path| seen.insert(path.clone())),
        );
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RecordPage {
    pub(crate) pinned: Vec<EntryRecord>,
    pub(crate) normal: Vec<EntryRecord>,
    pub(crate) next_cursor: Option<ClipboardQueryCursor>,
    pub(crate) pinned_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PinToggleResult {
    Updated {
        is_pinned: bool,
        mutation: MutationResult,
    },
    NotFound,
    LimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplacePreviewResult {
    pub(crate) old_path: Option<String>,
}

/// The engine-owned clipboard repository.
///
/// `Connection` deliberately has no mutex: this type is created and used only
/// on the single clipboard engine thread.
pub(crate) struct Repository {
    conn: Connection,
    schema_rebuilt: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PinScope {
    Pinned,
    Normal,
}

impl Repository {
    pub(crate) fn open(
        db_path: &Path,
        raw_key_hex: &str,
        recreate_on_first_key: bool,
    ) -> Result<Self, String> {
        validate_raw_key(raw_key_hex)?;
        let existed_before_open = db_path.exists();
        let mut conn = Self::open_encrypted(db_path, raw_key_hex).or_else(|error| {
            if recreate_on_first_key
                && existed_before_open
                && Self::is_unrecoverable_decrypt_error(&error)
            {
                warn!(
                    "Recreating encrypted clipboard database after initial open failure: {error}"
                );
                Self::remove_database_files(db_path)?;
                Self::open_encrypted(db_path, raw_key_hex)
            } else {
                Err(error)
            }
        })?;

        let version = conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .map_err(|error| format!("Failed to read clipboard schema version: {error}"))?;
        let schema_rebuilt = version != SCHEMA_VERSION;
        if schema_rebuilt {
            // Clipboard schema changes are intentionally destructive. Closing
            // and replacing the whole encrypted database avoids carrying any
            // knowledge of legacy tables or migration order into v9.
            drop(conn);
            Self::remove_database_files(db_path)?;
            conn = Self::open_encrypted(db_path, raw_key_hex)?;
        }
        Self::ensure_schema(&conn)?;

        Ok(Self {
            conn,
            schema_rebuilt,
        })
    }

    pub(crate) fn schema_rebuilt(&self) -> bool {
        self.schema_rebuilt
    }

    pub(crate) fn insert_entry(
        &mut self,
        entry: NewEntry,
        window_start: Option<i64>,
        max_history: u32,
    ) -> Result<MutationResult, String> {
        validate_new_entry(&entry)?;

        let tx = self
            .conn
            .transaction()
            .map_err(|error| format!("Failed to begin clipboard insert: {error}"))?;
        tx.execute(
            "INSERT INTO clipboard_entries
             (id, content_type, content, canonical_search_text, created_at, is_pinned,
              source_app, original_rel_path, preview_rel_path)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8)",
            params![
                &entry.id,
                entry.content_type.as_str(),
                &entry.content,
                &entry.canonical_search_text,
                entry.created_at,
                &entry.source_app,
                &entry.original_rel_path,
                &entry.preview_rel_path,
            ],
        )
        .map_err(|error| format!("Failed to insert clipboard entry: {error}"))?;
        Self::insert_tags_on(&tx, &entry.id, &entry.tags)?;

        let mutation = Self::prune_on(&tx, window_start, max_history, Some(&entry.id))?.inserted();
        tx.commit()
            .map_err(|error| format!("Failed to commit clipboard insert: {error}"))?;
        Ok(mutation)
    }

    pub(crate) fn list_records(
        &self,
        query: &ClipboardEntriesQuery,
        window_start: Option<i64>,
    ) -> Result<RecordPage, String> {
        let pinned = if query.cursor.is_none() {
            self.query_records(query, window_start, PinScope::Pinned, None)?
        } else {
            Vec::new()
        };

        let limit = query.normalized_limit() as usize;
        let mut normal =
            self.query_records(query, window_start, PinScope::Normal, Some(limit + 1))?;
        let has_more = normal.len() > limit;
        if has_more {
            normal.truncate(limit);
        }
        let next_cursor =
            has_more
                .then(|| normal.last())
                .flatten()
                .map(|entry| ClipboardQueryCursor {
                    created_at: entry.created_at,
                    id: entry.id.clone(),
                });

        Ok(RecordPage {
            pinned,
            normal,
            next_cursor,
            pinned_count: self.pinned_count()?,
        })
    }

    pub(crate) fn get_entry(&self, id: &str) -> Result<Option<EntryRecord>, String> {
        let mut entry = self
            .conn
            .query_row(
                &format!("{ENTRY_SELECT} WHERE e.id = ?1 LIMIT 1"),
                params![id],
                row_to_entry,
            )
            .optional()
            .map_err(|error| format!("Failed to read clipboard entry: {error}"))?;

        if let Some(entry) = entry.as_mut() {
            Self::load_tags_on(&self.conn, std::slice::from_mut(entry))?;
        }
        Ok(entry)
    }

    pub(crate) fn delete_entry(&mut self, id: &str) -> Result<MutationResult, String> {
        let tx = self
            .conn
            .transaction()
            .map_err(|error| format!("Failed to begin clipboard deletion: {error}"))?;
        let mutation = Self::remove_ids_on(&tx, &[id.to_string()])?;
        tx.commit()
            .map_err(|error| format!("Failed to commit clipboard deletion: {error}"))?;
        Ok(mutation)
    }

    pub(crate) fn clear(&mut self) -> Result<MutationResult, String> {
        let tx = self
            .conn
            .transaction()
            .map_err(|error| format!("Failed to begin clipboard clear: {error}"))?;
        let ids = {
            let mut statement = tx
                .prepare("SELECT id FROM clipboard_entries ORDER BY created_at ASC, id ASC")
                .map_err(|error| format!("Failed to prepare clipboard clear: {error}"))?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| format!("Failed to query clipboard clear: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("Failed to read clipboard clear candidates: {error}"))?;
            ids
        };
        let mutation = Self::remove_ids_on(&tx, &ids)?;
        tx.commit()
            .map_err(|error| format!("Failed to commit clipboard clear: {error}"))?;
        Ok(mutation)
    }

    pub(crate) fn toggle_pin(
        &mut self,
        id: &str,
        max_pinned: u32,
        window_start: Option<i64>,
        max_history: u32,
    ) -> Result<PinToggleResult, String> {
        let tx = self
            .conn
            .transaction()
            .map_err(|error| format!("Failed to begin pin update: {error}"))?;
        let current = tx
            .query_row(
                "SELECT is_pinned FROM clipboard_entries WHERE id = ?1",
                params![id],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map_err(|error| format!("Failed to read pin state: {error}"))?;
        let Some(current) = current else {
            return Ok(PinToggleResult::NotFound);
        };

        let is_pinned = !current;
        if is_pinned {
            let count = Self::pinned_count_on(&tx)?;
            if count >= max_pinned {
                return Ok(PinToggleResult::LimitExceeded);
            }
        }

        tx.execute(
            "UPDATE clipboard_entries SET is_pinned = ?1 WHERE id = ?2",
            params![is_pinned, id],
        )
        .map_err(|error| format!("Failed to update pin state: {error}"))?;

        let mut mutation = MutationResult {
            changed: true,
            ..MutationResult::default()
        };
        if !is_pinned {
            mutation.merge(Self::prune_on(&tx, window_start, max_history, None)?);
        }

        tx.commit()
            .map_err(|error| format!("Failed to commit pin update: {error}"))?;
        Ok(PinToggleResult::Updated {
            is_pinned,
            mutation,
        })
    }

    pub(crate) fn prune(
        &mut self,
        window_start: Option<i64>,
        max_history: u32,
    ) -> Result<MutationResult, String> {
        let tx = self
            .conn
            .transaction()
            .map_err(|error| format!("Failed to begin retention prune: {error}"))?;
        let mutation = Self::prune_on(&tx, window_start, max_history, None)?;
        tx.commit()
            .map_err(|error| format!("Failed to commit retention prune: {error}"))?;
        Ok(mutation)
    }

    pub(crate) fn replace_preview_path(
        &mut self,
        id: &str,
        new_rel_path: &str,
    ) -> Result<Option<ReplacePreviewResult>, String> {
        if new_rel_path.trim().is_empty() {
            return Err("Preview path cannot be empty".to_string());
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|error| format!("Failed to begin preview update: {error}"))?;
        let old_path = tx
            .query_row(
                "SELECT preview_rel_path FROM clipboard_entries
                 WHERE id = ?1 AND content_type = 'image'",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| format!("Failed to read preview path: {error}"))?;
        let Some(old_path) = old_path else {
            return Ok(None);
        };

        let changed = old_path.as_deref() != Some(new_rel_path);
        if changed {
            tx.execute(
                "UPDATE clipboard_entries SET preview_rel_path = ?1 WHERE id = ?2",
                params![new_rel_path, id],
            )
            .map_err(|error| format!("Failed to update preview path: {error}"))?;
        }
        tx.commit()
            .map_err(|error| format!("Failed to commit preview update: {error}"))?;

        Ok(Some(ReplacePreviewResult {
            old_path: changed.then_some(old_path).flatten(),
        }))
    }

    pub(crate) fn pinned_count(&self) -> Result<u32, String> {
        Self::pinned_count_on(&self.conn)
    }

    pub(crate) fn earliest_month(
        &self,
        window_start: Option<i64>,
    ) -> Result<Option<String>, String> {
        let mut values = Vec::new();
        let visibility = visibility_condition(window_start, &mut values);
        let sql = format!(
            "SELECT strftime('%Y-%m', e.created_at, 'unixepoch', 'localtime')
             FROM clipboard_entries e
             WHERE {visibility}
             ORDER BY e.created_at ASC, e.id ASC
             LIMIT 1"
        );
        self.conn
            .query_row(&sql, rusqlite::params_from_iter(values.iter()), |row| {
                row.get(0)
            })
            .optional()
            .map_err(|error| format!("Failed to read earliest clipboard month: {error}"))
    }

    pub(crate) fn active_dates(
        &self,
        year_month: &str,
        window_start: Option<i64>,
    ) -> Result<Vec<String>, String> {
        let mut values = vec![Value::Text(year_month.to_string())];
        let visibility = visibility_condition(window_start, &mut values);
        let sql = format!(
            "SELECT DISTINCT date(e.created_at, 'unixepoch', 'localtime') AS local_date
             FROM clipboard_entries e
             WHERE strftime('%Y-%m', e.created_at, 'unixepoch', 'localtime') = ?
               AND {visibility}
             ORDER BY local_date ASC"
        );
        let mut statement = self
            .conn
            .prepare(&sql)
            .map_err(|error| format!("Failed to prepare active clipboard dates: {error}"))?;
        let dates = statement
            .query_map(rusqlite::params_from_iter(values.iter()), |row| row.get(0))
            .map_err(|error| format!("Failed to query active clipboard dates: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Failed to read active clipboard dates: {error}"))?;
        Ok(dates)
    }

    pub(crate) fn referenced_paths(&self) -> Result<HashSet<String>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT original_rel_path, preview_rel_path
                 FROM clipboard_entries
                 WHERE original_rel_path IS NOT NULL OR preview_rel_path IS NOT NULL",
            )
            .map_err(|error| format!("Failed to prepare artifact path query: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .map_err(|error| format!("Failed to query artifact paths: {error}"))?;

        let mut paths = HashSet::new();
        for row in rows {
            let (original, preview) =
                row.map_err(|error| format!("Failed to read artifact paths: {error}"))?;
            paths.extend(original);
            paths.extend(preview);
        }
        Ok(paths)
    }

    fn open_encrypted(db_path: &Path, raw_key_hex: &str) -> Result<Connection, String> {
        let conn = Connection::open(db_path)
            .map_err(|error| format!("Failed to open clipboard database: {error}"))?;
        conn.execute_batch(&format!("PRAGMA key = \"x'{raw_key_hex}'\";"))
            .map_err(|error| format!("Failed to apply clipboard database key: {error}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=3000;
             PRAGMA foreign_keys=ON;",
        )
        .map_err(|error| format!("Failed to configure clipboard database: {error}"))?;
        conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| format!("Failed to unlock encrypted database: {error}"))?;

        Ok(conn)
    }

    fn ensure_schema(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS clipboard_entries (
                 id                    TEXT PRIMARY KEY,
                 content_type          TEXT NOT NULL CHECK(content_type IN ('text', 'image')),
                 content               TEXT NOT NULL DEFAULT '',
                 canonical_search_text TEXT NOT NULL DEFAULT '',
                 created_at            INTEGER NOT NULL,
                 is_pinned             INTEGER NOT NULL DEFAULT 0 CHECK(is_pinned IN (0, 1)),
                 source_app            TEXT NOT NULL DEFAULT '',
                 original_rel_path     TEXT,
                 preview_rel_path      TEXT,
                 CHECK (
                     (content_type = 'text'
                      AND original_rel_path IS NULL
                      AND preview_rel_path IS NULL)
                     OR
                     (content_type = 'image'
                      AND original_rel_path IS NOT NULL
                      AND preview_rel_path IS NOT NULL)
                 )
             );
             CREATE TABLE IF NOT EXISTS clipboard_entry_tags (
                 entry_id TEXT NOT NULL,
                 tag      TEXT NOT NULL,
                 PRIMARY KEY(entry_id, tag),
                 FOREIGN KEY(entry_id) REFERENCES clipboard_entries(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_clipboard_normal_cursor
                 ON clipboard_entries(created_at DESC, id DESC)
                 WHERE is_pinned = 0;
             CREATE INDEX IF NOT EXISTS idx_clipboard_pinned_cursor
                 ON clipboard_entries(created_at DESC, id DESC)
                 WHERE is_pinned = 1;
             CREATE INDEX IF NOT EXISTS idx_clipboard_retention
                 ON clipboard_entries(created_at ASC, id ASC)
                 WHERE is_pinned = 0;
             CREATE INDEX IF NOT EXISTS idx_clipboard_content_type
                 ON clipboard_entries(content_type);
             CREATE INDEX IF NOT EXISTS idx_clipboard_tags_value
                 ON clipboard_entry_tags(tag, entry_id);
             PRAGMA user_version = 9;
             COMMIT;",
        )
        .map_err(|error| format!("Failed to create clipboard schema indexes: {error}"))?;
        Ok(())
    }

    fn remove_database_files(db_path: &Path) -> Result<(), String> {
        for suffix in ["", "-wal", "-shm"] {
            let mut candidate = db_path.as_os_str().to_os_string();
            candidate.push(suffix);
            let candidate = std::path::PathBuf::from(candidate);
            if candidate.exists() {
                std::fs::remove_file(&candidate).map_err(|error| {
                    format!(
                        "Failed to recreate encrypted clipboard database at {}: {error}",
                        candidate.display()
                    )
                })?;
            }
        }
        Ok(())
    }

    fn is_unrecoverable_decrypt_error(error: &str) -> bool {
        let error = error.to_ascii_lowercase();
        error.contains("file is encrypted") || error.contains("not a database")
    }

    fn insert_tags_on(conn: &Connection, entry_id: &str, tags: &[String]) -> Result<(), String> {
        let mut statement = conn
            .prepare(
                "INSERT INTO clipboard_entry_tags (entry_id, tag)
                 VALUES (?1, ?2)",
            )
            .map_err(|error| format!("Failed to prepare clipboard tags: {error}"))?;
        let mut seen = HashSet::new();
        for tag in tags
            .iter()
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
        {
            if seen.insert(tag) {
                statement
                    .execute(params![entry_id, tag])
                    .map_err(|error| format!("Failed to insert clipboard tag: {error}"))?;
            }
        }
        Ok(())
    }

    fn query_records(
        &self,
        query: &ClipboardEntriesQuery,
        window_start: Option<i64>,
        scope: PinScope,
        limit: Option<usize>,
    ) -> Result<Vec<EntryRecord>, String> {
        let mut conditions = Vec::new();
        let mut values = Vec::new();
        append_query_filters(&mut conditions, &mut values, query, window_start, scope);

        if scope == PinScope::Normal {
            if let Some(cursor) = query.cursor.as_ref() {
                conditions
                    .push("(e.created_at < ? OR (e.created_at = ? AND e.id < ?))".to_string());
                values.push(Value::Integer(cursor.created_at));
                values.push(Value::Integer(cursor.created_at));
                values.push(Value::Text(cursor.id.clone()));
            }
        }

        let mut sql = format!(
            "{ENTRY_SELECT} WHERE {} ORDER BY e.created_at DESC, e.id DESC",
            conditions.join(" AND ")
        );
        if let Some(limit) = limit {
            sql.push_str(" LIMIT ?");
            values.push(Value::Integer(limit as i64));
        }

        let mut records = {
            let mut statement = self
                .conn
                .prepare(&sql)
                .map_err(|error| format!("Failed to prepare clipboard query: {error}"))?;
            let records = statement
                .query_map(rusqlite::params_from_iter(values.iter()), row_to_entry)
                .map_err(|error| format!("Failed to query clipboard entries: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("Failed to read clipboard entries: {error}"))?;
            records
        };
        Self::load_tags_on(&self.conn, &mut records)?;
        Ok(records)
    }

    fn load_tags_on(conn: &Connection, records: &mut [EntryRecord]) -> Result<(), String> {
        if records.is_empty() {
            return Ok(());
        }

        let placeholders = std::iter::repeat_n("?", records.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT entry_id, tag FROM clipboard_entry_tags
             WHERE entry_id IN ({placeholders})
             ORDER BY entry_id ASC, tag ASC"
        );
        let ids = records
            .iter()
            .map(|record| Value::Text(record.id.clone()))
            .collect::<Vec<_>>();
        let mut statement = conn
            .prepare(&sql)
            .map_err(|error| format!("Failed to prepare clipboard tag query: {error}"))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Failed to query clipboard tags: {error}"))?;

        let mut tags_by_id: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let (id, tag) =
                row.map_err(|error| format!("Failed to read clipboard tag: {error}"))?;
            tags_by_id.entry(id).or_default().push(tag);
        }
        for record in records {
            record.tags = tags_by_id.remove(&record.id).unwrap_or_default();
        }
        Ok(())
    }

    fn pinned_count_on(conn: &Connection) -> Result<u32, String> {
        let count = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_entries WHERE is_pinned = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| format!("Failed to count pinned clipboard entries: {error}"))?;
        u32::try_from(count).map_err(|_| "Pinned clipboard count exceeds u32".to_string())
    }

    fn prune_on(
        conn: &Connection,
        window_start: Option<i64>,
        max_history: u32,
        protected_id: Option<&str>,
    ) -> Result<MutationResult, String> {
        let mut mutation = MutationResult::default();

        if let Some(window_start) = window_start {
            let mut values = vec![Value::Integer(window_start)];
            let mut sql = "SELECT id FROM clipboard_entries WHERE is_pinned = 0 AND created_at < ?"
                .to_string();
            if let Some(protected_id) = protected_id {
                sql.push_str(" AND id != ?");
                values.push(Value::Text(protected_id.to_string()));
            }
            sql.push_str(" ORDER BY created_at ASC, id ASC");
            let expired = query_ids(conn, &sql, &values, "expired clipboard entries")?;
            mutation.merge(Self::remove_ids_on(conn, &expired)?);
        }

        let normal_count = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_entries WHERE is_pinned = 0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| format!("Failed to count clipboard history: {error}"))?;
        let excess = normal_count.saturating_sub(i64::from(max_history));
        if excess > 0 {
            let (sql, values) = if let Some(protected_id) = protected_id {
                (
                    "SELECT id FROM clipboard_entries
                     WHERE is_pinned = 0 AND id != ?
                     ORDER BY created_at ASC, id ASC LIMIT ?",
                    vec![
                        Value::Text(protected_id.to_string()),
                        Value::Integer(excess),
                    ],
                )
            } else {
                (
                    "SELECT id FROM clipboard_entries
                     WHERE is_pinned = 0
                     ORDER BY created_at ASC, id ASC LIMIT ?",
                    vec![Value::Integer(excess)],
                )
            };
            let trimmed = query_ids(conn, sql, &values, "clipboard history trim")?;
            mutation.merge(Self::remove_ids_on(conn, &trimmed)?);
        }

        Ok(mutation)
    }

    fn remove_ids_on(conn: &Connection, ids: &[String]) -> Result<MutationResult, String> {
        if ids.is_empty() {
            return Ok(MutationResult::default());
        }

        let mut paths_by_id = HashMap::new();
        for ids in ids.chunks(SQLITE_PARAM_BATCH) {
            let placeholders = std::iter::repeat_n("?", ids.len())
                .collect::<Vec<_>>()
                .join(", ");
            let values = ids.iter().cloned().map(Value::Text).collect::<Vec<_>>();
            let path_sql = format!(
                "SELECT id, original_rel_path, preview_rel_path
                 FROM clipboard_entries WHERE id IN ({placeholders})"
            );
            let mut statement = conn
                .prepare(&path_sql)
                .map_err(|error| format!("Failed to prepare removal paths: {error}"))?;
            let rows = statement
                .query_map(rusqlite::params_from_iter(values.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(|error| format!("Failed to query removal paths: {error}"))?;
            for row in rows {
                let (id, original, preview) =
                    row.map_err(|error| format!("Failed to read removal paths: {error}"))?;
                paths_by_id.insert(id, (original, preview));
            }
        }

        let removed_ids = ids
            .iter()
            .filter(|id| paths_by_id.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        if removed_ids.is_empty() {
            return Ok(MutationResult::default());
        }

        for ids in removed_ids.chunks(SQLITE_PARAM_BATCH) {
            let placeholders = std::iter::repeat_n("?", ids.len())
                .collect::<Vec<_>>()
                .join(", ");
            conn.execute(
                &format!("DELETE FROM clipboard_entries WHERE id IN ({placeholders})"),
                rusqlite::params_from_iter(ids.iter()),
            )
            .map_err(|error| format!("Failed to delete clipboard entries: {error}"))?;
        }

        let mut cleanup_paths = Vec::new();
        let mut seen_paths = HashSet::new();
        for id in &removed_ids {
            if let Some((original, preview)) = paths_by_id.get(id) {
                for path in original.iter().chain(preview.iter()) {
                    if seen_paths.insert(path.clone()) {
                        cleanup_paths.push(path.clone());
                    }
                }
            }
        }

        Ok(MutationResult {
            changed: true,
            cleanup_paths,
        })
    }
}

const ENTRY_SELECT: &str = "SELECT e.id, e.content_type, e.content, e.created_at,
            e.is_pinned, e.source_app, e.original_rel_path, e.preview_rel_path
     FROM clipboard_entries e";

fn append_query_filters(
    conditions: &mut Vec<String>,
    values: &mut Vec<Value>,
    query: &ClipboardEntriesQuery,
    window_start: Option<i64>,
    scope: PinScope,
) {
    match scope {
        PinScope::Pinned => conditions.push("e.is_pinned = 1".to_string()),
        PinScope::Normal => {
            conditions.push("e.is_pinned = 0".to_string());
            if let Some(window_start) = window_start {
                conditions.push("e.created_at >= ?".to_string());
                values.push(Value::Integer(window_start));
            }
        }
    }

    if let Some(content_type) = query.entry_type {
        conditions.push("e.content_type = ?".to_string());
        values.push(Value::Text(content_type.as_str().to_string()));
    }
    if let Some(text) = query.text().and_then(canonicalize_query_text) {
        conditions.push("e.content_type = 'text'".to_string());
        conditions.push("e.canonical_search_text LIKE ? ESCAPE '\\'".to_string());
        values.push(Value::Text(format!("%{}%", escape_like(&text))));
    }
    if let Some(date) = query.date() {
        conditions.push("date(e.created_at, 'unixepoch', 'localtime') = ?".to_string());
        values.push(Value::Text(date.to_string()));
    }
    if let Some(tag) = query.tag() {
        conditions.push(
            "EXISTS (
                 SELECT 1 FROM clipboard_entry_tags t
                 WHERE t.entry_id = e.id AND t.tag = ?
             )"
            .to_string(),
        );
        values.push(Value::Text(tag.to_string()));
    }
}

fn visibility_condition(window_start: Option<i64>, values: &mut Vec<Value>) -> String {
    if let Some(window_start) = window_start {
        values.push(Value::Integer(window_start));
        "(e.is_pinned = 1 OR e.created_at >= ?)".to_string()
    } else {
        "1 = 1".to_string()
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn query_ids(
    conn: &Connection,
    sql: &str,
    values: &[Value],
    operation: &str,
) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| format!("Failed to prepare {operation}: {error}"))?;
    let ids = statement
        .query_map(rusqlite::params_from_iter(values.iter()), |row| row.get(0))
        .map_err(|error| format!("Failed to query {operation}: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Failed to read {operation}: {error}"))?;
    Ok(ids)
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntryRecord> {
    let content_type = row.get::<_, String>(1)?;
    Ok(EntryRecord {
        id: row.get(0)?,
        content_type: ClipboardContentType::from_db(&content_type).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        })?,
        content: row.get(2)?,
        created_at: row.get(3)?,
        is_pinned: row.get::<_, bool>(4)?,
        source_app: row.get(5)?,
        original_rel_path: row.get(6)?,
        preview_rel_path: row.get(7)?,
        tags: Vec::new(),
    })
}

fn validate_raw_key(raw_key_hex: &str) -> Result<(), String> {
    if raw_key_hex.len() != 64 || !raw_key_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Clipboard database key must be 32 raw bytes encoded as hex".to_string());
    }
    Ok(())
}

fn validate_new_entry(entry: &NewEntry) -> Result<(), String> {
    if entry.id.is_empty() {
        return Err("Clipboard entry id cannot be empty".to_string());
    }
    match entry.content_type {
        ClipboardContentType::Text => {
            if entry.original_rel_path.is_some() || entry.preview_rel_path.is_some() {
                return Err("Text clipboard entries cannot reference image artifacts".to_string());
            }
        }
        ClipboardContentType::Image => {
            if entry.original_rel_path.as_deref().is_none_or(str::is_empty)
                || entry.preview_rel_path.as_deref().is_none_or(str::is_empty)
            {
                return Err(
                    "Image clipboard entries require ready original and preview paths".to_string(),
                );
            }
        }
    }
    Ok(())
}
