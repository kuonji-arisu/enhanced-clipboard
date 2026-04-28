use log::warn;
use rusqlite::{params, types::Value, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use crate::models::{ClipboardEntriesQuery, ClipboardEntry};
use crate::services::search_preview::canonicalize_query_text;

/// 当前 DB schema 版本；schema 变更时递增，旧版本会被自动清空重建。
const SCHEMA_VERSION: u32 = 4;

/// 仅管理剪贴板记录与其附属属性表。
pub struct Database {
    conn: Mutex<Connection>,
}

pub enum PinToggleResult {
    Updated(bool),
    NotFound,
    LimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageAssetRecord {
    pub id: String,
    pub image_path: Option<String>,
    pub thumbnail_path: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PinScope {
    PinnedOnly,
    NonPinnedOnly,
    Visible,
}

impl Database {
    fn insert_entry_on(conn: &Connection, entry: &ClipboardEntry) -> Result<(), String> {
        conn.execute(
            "INSERT INTO clipboard_entries \
             (id, content_type, content, canonical_search_text, created_at, is_pinned, source_app, image_path, thumbnail_path) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.id,
                entry.content_type,
                entry.content,
                entry.canonical_search_text,
                entry.created_at,
                entry.is_pinned as i32,
                entry.source_app,
                entry.image_path,
                entry.thumbnail_path,
            ],
        )
        .map_err(|e| format!("Failed to insert entry: {}", e))?;
        Ok(())
    }

    fn replace_entry_attrs_on(
        conn: &Connection,
        entry_id: &str,
        attr_type: &str,
        values: &[String],
    ) -> Result<(), String> {
        conn.execute(
            "DELETE FROM clipboard_entry_attrs WHERE entry_id = ?1 AND attr_type = ?2",
            params![entry_id, attr_type],
        )
        .map_err(|e| format!("Failed to delete entry attrs: {}", e))?;

        let mut seen = HashSet::new();
        let mut stmt = conn
            .prepare(
                "INSERT INTO clipboard_entry_attrs (entry_id, attr_type, attr_value)
                 VALUES (?1, ?2, ?3)",
            )
            .map_err(|e| format!("Failed to prepare entry attrs insert: {}", e))?;

        for value in values {
            if value.is_empty() || !seen.insert(value.clone()) {
                continue;
            }

            stmt.execute(params![entry_id, attr_type, value])
                .map_err(|e| format!("Failed to insert entry attrs: {}", e))?;
        }

        Ok(())
    }

    fn append_entry_query_filters(
        conditions: &mut Vec<String>,
        params: &mut Vec<Value>,
        query: &ClipboardEntriesQuery,
        window_start: i64,
        pin_scope: PinScope,
    ) {
        conditions.push(
            match pin_scope {
                PinScope::PinnedOnly => "is_pinned = 1",
                PinScope::NonPinnedOnly => "is_pinned = 0",
                PinScope::Visible if window_start > 0 => "(is_pinned = 1 OR created_at >= ?)",
                PinScope::Visible => "1 = 1",
            }
            .to_string(),
        );
        if pin_scope == PinScope::Visible && window_start > 0 {
            params.push(Value::Integer(window_start));
        }

        if pin_scope == PinScope::NonPinnedOnly && window_start > 0 {
            conditions.push("created_at >= ?".to_string());
            params.push(Value::Integer(window_start));
        }

        if let Some(entry_type) = query.entry_type() {
            conditions.push("content_type = ?".to_string());
            params.push(Value::Text(entry_type.as_str().to_string()));
        }

        if let Some(q) = query.text() {
            if query.entry_type().is_none() {
                conditions.push("content_type = 'text'".to_string());
            }
            if let Some(canonical_query) = canonicalize_query_text(q) {
                let like_p = format!(
                    "%{}%",
                    canonical_query
                        .replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                );
                conditions.push("canonical_search_text LIKE ? ESCAPE '\\'".to_string());
                params.push(Value::Text(like_p));
            }
        }

        if let Some(date) = query.date() {
            conditions.push("date(created_at, 'unixepoch', 'localtime') = ?".to_string());
            params.push(Value::Text(date.to_string()));
        }

        if let Some(tag) = query.tag() {
            conditions.push(
                "EXISTS (
                     SELECT 1
                     FROM clipboard_entry_attrs a
                     WHERE a.entry_id = clipboard_entries.id
                       AND a.attr_type = 'tag'
                       AND a.attr_value = ?
                 )"
                .to_string(),
            );
            params.push(Value::Text(tag.to_string()));
        }
    }

    fn append_cursor_filter(
        conditions: &mut Vec<String>,
        params: &mut Vec<Value>,
        query: &ClipboardEntriesQuery,
    ) {
        if let Some(cursor) = query.cursor.as_ref() {
            conditions.push("(created_at < ? OR (created_at = ? AND id < ?))".to_string());
            params.push(Value::Integer(cursor.created_at));
            params.push(Value::Integer(cursor.created_at));
            params.push(Value::Text(cursor.id.clone()));
        }
    }

    pub fn new(
        db_path: &str,
        raw_key_hex: &str,
        recreate_on_first_key: bool,
    ) -> Result<Self, String> {
        let conn = Self::open_encrypted(db_path, raw_key_hex).or_else(|err| {
            if Self::should_recreate_after_open_failure(db_path, &err, recreate_on_first_key) {
                warn!(
                    "Recreating encrypted clipboard database after initial open failure: {}",
                    err
                );
                Self::recreate_encrypted(db_path, raw_key_hex)
            } else {
                Err(err)
            }
        })?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn should_recreate_after_open_failure(
        db_path: &str,
        err: &str,
        recreate_on_first_key: bool,
    ) -> bool {
        recreate_on_first_key
            && Path::new(db_path).exists()
            && Self::is_unrecoverable_decrypt_error(err)
    }

    fn is_unrecoverable_decrypt_error(err: &str) -> bool {
        err.contains("Failed to unlock encrypted database")
            || err.contains("file is not a database")
            || err.contains("not a database")
    }

    fn open_encrypted(db_path: &str, raw_key_hex: &str) -> Result<Connection, String> {
        let conn =
            Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        Self::apply_raw_key(&conn, raw_key_hex)?;
        Self::configure_connection(&conn)?;
        Self::verify_key(&conn)?;
        Self::ensure_schema(&conn)?;
        Ok(conn)
    }

    fn recreate_encrypted(db_path: &str, raw_key_hex: &str) -> Result<Connection, String> {
        Self::remove_database_files(db_path)?;
        Self::open_encrypted(db_path, raw_key_hex)
    }

    fn apply_raw_key(conn: &Connection, raw_key_hex: &str) -> Result<(), String> {
        conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", raw_key_hex))
            .map_err(|e| format!("Failed to apply database encryption key: {}", e))
    }

    fn configure_connection(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=3000; PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| format!("PRAGMA init failed: {}", e))
    }

    fn verify_key(conn: &Connection) -> Result<(), String> {
        conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|_| ())
        .map_err(|e| format!("Failed to unlock encrypted database: {}", e))
    }

    fn ensure_schema(conn: &Connection) -> Result<(), String> {
        // Schema 版本检查：版本不匹配时清空旧表，无需迁移
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if version < SCHEMA_VERSION {
            conn.execute_batch(
                "DROP TABLE IF EXISTS clipboard_entry_attrs;
                 DROP TABLE IF EXISTS clipboard_entries;",
            )
            .map_err(|e| format!("Failed to drop old tables: {}", e))?;
            conn.execute(&format!("PRAGMA user_version = {}", SCHEMA_VERSION), [])
                .map_err(|e| format!("Failed to set schema version: {}", e))?;
        }

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS clipboard_entries (
                 id             TEXT PRIMARY KEY,
                 content_type   TEXT NOT NULL,
                 content        TEXT NOT NULL DEFAULT '',
                 canonical_search_text TEXT NOT NULL DEFAULT '',
                 created_at     INTEGER NOT NULL,
                 is_pinned      INTEGER DEFAULT 0,
                 source_app     TEXT DEFAULT '',
                 image_path     TEXT,
                 thumbnail_path TEXT
             );
             CREATE TABLE IF NOT EXISTS clipboard_entry_attrs (
                 entry_id   TEXT NOT NULL,
                 attr_type  TEXT NOT NULL,
                 attr_value TEXT NOT NULL,
                 PRIMARY KEY (entry_id, attr_type, attr_value),
                 FOREIGN KEY (entry_id) REFERENCES clipboard_entries(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_created_at
                 ON clipboard_entries(created_at);
             CREATE INDEX IF NOT EXISTS idx_normal_cursor
                 ON clipboard_entries(created_at DESC, id DESC)
                 WHERE is_pinned = 0;
             CREATE INDEX IF NOT EXISTS idx_entry_attrs_type_value
                 ON clipboard_entry_attrs(attr_type, attr_value);
             CREATE INDEX IF NOT EXISTS idx_entry_attrs_entry_id
                 ON clipboard_entry_attrs(entry_id);",
        )
        .map_err(|e| format!("Failed to create tables: {}", e))?;

        Ok(())
    }

    fn remove_database_files(db_path: &str) -> Result<(), String> {
        for suffix in ["", "-wal", "-shm"] {
            let path = format!("{db_path}{suffix}");
            let path = Path::new(&path);
            if path.exists() {
                std::fs::remove_file(path)
                    .map_err(|e| format!("Failed to recreate encrypted database: {}", e))?;
            }
        }
        Ok(())
    }

    pub fn insert_entry(&self, entry: &ClipboardEntry) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        Self::insert_entry_on(&conn, entry)
    }

    pub fn insert_entry_with_attrs(
        &self,
        entry: &ClipboardEntry,
        attrs: &[(&str, &[String])],
    ) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        Self::insert_entry_on(&tx, entry)?;
        for (attr_type, values) in attrs {
            Self::replace_entry_attrs_on(&tx, &entry.id, attr_type, values)?;
        }

        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 保留给后续条目 attrs 更新场景复用；当前写入主路径由 `insert_entry_with_attrs` 使用事务封装。
    #[allow(dead_code)]
    pub fn replace_entry_attrs(
        &self,
        entry_id: &str,
        attr_type: &str,
        values: &[String],
    ) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        Self::replace_entry_attrs_on(&tx, entry_id, attr_type, values)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_entry_attrs_for_ids(
        &self,
        entry_ids: &[String],
        attr_type: &str,
    ) -> Result<HashMap<String, Vec<String>>, String> {
        if entry_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let placeholders = entry_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT entry_id, attr_value
             FROM clipboard_entry_attrs
             WHERE attr_type = ? AND entry_id IN ({})
             ORDER BY entry_id ASC, attr_value ASC",
            placeholders
        );

        let mut params: Vec<Value> = Vec::with_capacity(entry_ids.len() + 1);
        params.push(Value::Text(attr_type.to_string()));
        params.extend(entry_ids.iter().cloned().map(Value::Text));

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;

        let mut attrs = HashMap::new();
        for row in rows {
            let (entry_id, value) = row.map_err(|e| e.to_string())?;
            attrs.entry(entry_id).or_insert_with(Vec::new).push(value);
        }
        Ok(attrs)
    }

    /// 所有命中的置顶条目（不受 TTL 限制，不分页）。
    pub fn get_pinned(&self, query: &ClipboardEntriesQuery) -> Result<Vec<ClipboardEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        Self::append_entry_query_filters(
            &mut conditions,
            &mut params,
            query,
            0,
            PinScope::PinnedOnly,
        );

        let mut stmt = conn
            .prepare(
                &format!(
                    "SELECT id, content_type, content, canonical_search_text, created_at, is_pinned, source_app, image_path, thumbnail_path
                     FROM clipboard_entries
                     WHERE {}
                     ORDER BY created_at DESC, id DESC",
                    conditions.join(" AND ")
                ),
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params), row_to_entry)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// 非置顶条目的游标分页查询（复合 cursor，解决同秒冲突）。
    /// - `window_start`: 时间窗口起点（epoch 秒），0 表示不限
    /// - `cursor_ts` / `cursor_id`: 上一页最后一条的 (created_at, id)；None 表示首页
    pub fn get_normal_page(
        &self,
        query: &ClipboardEntriesQuery,
        window_start: i64,
    ) -> Result<Vec<ClipboardEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        let mut conditions: Vec<String> = Vec::new();
        let mut p: Vec<Value> = Vec::new();

        Self::append_entry_query_filters(
            &mut conditions,
            &mut p,
            query,
            window_start,
            PinScope::NonPinnedOnly,
        );
        Self::append_cursor_filter(&mut conditions, &mut p, query);

        p.push(Value::Integer(query.normalized_limit() as i64));

        let sql = format!(
            "SELECT id, content_type, content, canonical_search_text, created_at, is_pinned, source_app, image_path, thumbnail_path
             FROM clipboard_entries
             WHERE {}
             ORDER BY created_at DESC, id DESC LIMIT ?",
            conditions.join(" AND ")
        );

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(p), row_to_entry)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// 非置顶条目总数（用于 prune 缓冲区判断）。
    pub fn count_normal(&self) -> Result<u32, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_entries WHERE is_pinned = 0",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(count)
    }

    /// 返回当前可见视图中最早有记录的年月（YYYY-MM 格式），表为空时返回 None。
    /// 置顶条目始终可见；非置顶条目仅在 TTL 窗口内可见。
    pub fn get_earliest_month(&self, window_start: i64) -> Result<Option<String>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let result: Option<String> = if window_start > 0 {
            conn.query_row(
                "SELECT strftime('%Y-%m', MIN(created_at), 'unixepoch', 'localtime')
                 FROM clipboard_entries
                 WHERE is_pinned = 1 OR created_at >= ?1",
                params![window_start],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?
        } else {
            conn.query_row(
                "SELECT strftime('%Y-%m', MIN(created_at), 'unixepoch', 'localtime') FROM clipboard_entries",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?
        };
        Ok(result)
    }

    /// 返回当前可见视图中指定年月（YYYY-MM）内有记录的日期列表（YYYY-MM-DD 格式）。
    /// 置顶条目始终可见；非置顶条目仅在 TTL 窗口内可见。
    pub fn get_active_dates_in_month(
        &self,
        year_month: &str,
        window_start: i64,
    ) -> Result<Vec<String>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let (sql, params_vec): (&str, Vec<Value>) = if window_start > 0 {
            (
                "SELECT DISTINCT date(created_at, 'unixepoch', 'localtime')
                 FROM clipboard_entries
                 WHERE strftime('%Y-%m', created_at, 'unixepoch', 'localtime') = ?1
                   AND (is_pinned = 1 OR created_at >= ?2)
                 ORDER BY 1",
                vec![
                    Value::Text(year_month.to_string()),
                    Value::Integer(window_start),
                ],
            )
        } else {
            (
                "SELECT DISTINCT date(created_at, 'unixepoch', 'localtime')
                 FROM clipboard_entries
                 WHERE strftime('%Y-%m', created_at, 'unixepoch', 'localtime') = ?1
                 ORDER BY 1",
                vec![Value::Text(year_month.to_string())],
            )
        };
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_vec), |row| row.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn get_entry_by_id(&self, id: &str) -> Result<Option<ClipboardEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, content_type, content, canonical_search_text, created_at, is_pinned, source_app, image_path, thumbnail_path
                 FROM clipboard_entries WHERE id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let entry = stmt
            .query_row(params![id], row_to_entry)
            .optional()
            .map_err(|e| e.to_string())?;
        Ok(entry)
    }

    pub fn get_entry_by_id_for_query(
        &self,
        id: &str,
        query: &ClipboardEntriesQuery,
        window_start: i64,
    ) -> Result<Option<ClipboardEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut conditions = vec!["id = ?".to_string()];
        let mut params = vec![Value::Text(id.to_string())];

        Self::append_entry_query_filters(
            &mut conditions,
            &mut params,
            query,
            window_start,
            PinScope::Visible,
        );

        let sql = format!(
            "SELECT id, content_type, content, canonical_search_text, created_at, is_pinned, source_app, image_path, thumbnail_path
             FROM clipboard_entries
             WHERE {}
             LIMIT 1",
            conditions.join(" AND ")
        );

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let entry = stmt
            .query_row(rusqlite::params_from_iter(params), row_to_entry)
            .optional()
            .map_err(|e| e.to_string())?;
        Ok(entry)
    }

    /// 图片文件写入完成后，原子地更新路径并返回最终记录。
    /// 返回 `Ok(None)` 表示条目已不存在，调用方应安静清理刚写出的文件。
    pub fn finalize_image_entry(
        &self,
        id: &str,
        image_path: &str,
        thumbnail_path: Option<&str>,
    ) -> Result<Option<ClipboardEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let affected = conn
            .execute(
                "UPDATE clipboard_entries SET image_path = ?1, thumbnail_path = ?2 WHERE id = ?3",
                params![image_path, thumbnail_path, id],
            )
            .map_err(|e| e.to_string())?;
        if affected == 0 {
            return Ok(None);
        }

        let mut stmt = conn
            .prepare(
                "SELECT id, content_type, content, canonical_search_text, created_at, is_pinned, source_app, image_path, thumbnail_path
                 FROM clipboard_entries WHERE id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let entry = stmt
            .query_row(params![id], row_to_entry)
            .map_err(|e| format!("Failed to load finalized image entry: {}", e))?;
        Ok(Some(entry))
    }

    pub fn update_image_thumbnail_path(
        &self,
        id: &str,
        thumbnail_path: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE clipboard_entries SET thumbnail_path = ?1 WHERE id = ?2",
            params![thumbnail_path, id],
        )
        .map_err(|e| format!("Failed to update image thumbnail path: {}", e))?;
        Ok(())
    }

    pub fn clear_image_thumbnail_path(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE clipboard_entries SET thumbnail_path = NULL WHERE id = ?1",
            params![id],
        )
        .map_err(|e| format!("Failed to clear image thumbnail path: {}", e))?;
        Ok(())
    }

    pub fn get_image_asset_records(&self) -> Result<Vec<ImageAssetRecord>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, image_path, thumbnail_path
                 FROM clipboard_entries
                 WHERE content_type = 'image'",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ImageAssetRecord {
                    id: row.get(0)?,
                    image_path: row.get(1)?,
                    thumbnail_path: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn toggle_pinned_with_limit(
        &self,
        id: &str,
        max_pinned: u32,
    ) -> Result<PinToggleResult, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        let Some(current_pinned) = tx
            .query_row(
                "SELECT is_pinned FROM clipboard_entries WHERE id = ?1",
                params![id],
                |row| row.get::<_, i32>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
        else {
            return Ok(PinToggleResult::NotFound);
        };

        let new_state = current_pinned == 0;
        if new_state {
            let count: u32 = tx
                .query_row(
                    "SELECT COUNT(*) FROM clipboard_entries WHERE is_pinned = 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if count >= max_pinned {
                return Ok(PinToggleResult::LimitExceeded);
            }
        }

        tx.execute(
            "UPDATE clipboard_entries SET is_pinned = ?1 WHERE id = ?2",
            params![new_state as i32, id],
        )
        .map_err(|e| format!("Failed to update pin state: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(PinToggleResult::Updated(new_state))
    }

    pub fn delete_entry_with_assets(&self, id: &str) -> Result<Option<Vec<String>>, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        let paths = tx
            .query_row(
                "SELECT image_path, thumbnail_path FROM clipboard_entries WHERE id = ?1",
                params![id],
                |row| {
                    Ok([
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>())
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;

        if paths.is_none() {
            tx.rollback().map_err(|e| e.to_string())?;
            return Ok(None);
        }

        tx.execute("DELETE FROM clipboard_entries WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete entry: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(paths)
    }

    pub fn clear_all_with_assets(&self) -> Result<(Vec<String>, Vec<String>), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        let mut stmt = tx
            .prepare("SELECT id, image_path, thumbnail_path FROM clipboard_entries")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    [
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>(),
                ))
            })
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, Vec<String>)> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        drop(stmt);

        tx.execute("DELETE FROM clipboard_entries", [])
            .map_err(|e| format!("Failed to clear entries: {}", e))?;
        tx.commit().map_err(|e| e.to_string())?;
        let ids = rows.iter().map(|(id, _)| id.clone()).collect();
        let paths = rows.into_iter().flat_map(|(_, paths)| paths).collect();
        Ok((ids, paths))
    }

    pub fn delete_entries_with_assets(
        &self,
        ids: &[String],
    ) -> Result<(Vec<String>, Vec<String>), String> {
        if ids.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut stmt = tx
            .prepare(&format!(
                "SELECT id, image_path, thumbnail_path
                 FROM clipboard_entries
                 WHERE id IN ({})",
                placeholders
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    [
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>(),
                ))
            })
            .map_err(|e| e.to_string())?;
        let rows = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);

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

        let removed_ids = rows.iter().map(|(id, _)| id.clone()).collect();
        let paths = rows.into_iter().flat_map(|(_, paths)| paths).collect();
        Ok((removed_ids, paths))
    }

    pub fn delete_entry(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM clipboard_entries WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete entry: {}", e))?;
        Ok(())
    }

    /// 两步清理（在单个事务中执行）：
    /// Step 1：删除过期非置顶（created_at < window_start）
    /// Step 2：将非置顶数量截断至 max_entries（保留最新）
    /// 置顶条目永远不会被删除。
    /// 返回 (被删除的 id 列表, 需清理的文件相对路径列表)。
    pub fn prune(
        &self,
        window_start: i64,
        max_entries: u32,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        // ── Step 1：过期删除 ────────────────────────────────────────────
        let step1: Vec<(String, Option<String>, Option<String>)> = if window_start > 0 {
            let mut stmt = tx
                .prepare(
                    "SELECT id, image_path, thumbnail_path FROM clipboard_entries
                     WHERE is_pinned = 0 AND created_at < ?1",
                )
                .map_err(|e| e.to_string())?;
            let rows: Vec<_> = stmt
                .query_map(params![window_start], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            rows
        } else {
            vec![]
        };

        if !step1.is_empty() {
            tx.execute(
                "DELETE FROM clipboard_entries WHERE is_pinned = 0 AND created_at < ?1",
                params![window_start],
            )
            .map_err(|e| e.to_string())?;
        }

        // ── Step 2：数量截断（基于清理后的剩余数量） ─────────────────────
        let count_after: u32 = tx
            .query_row(
                "SELECT COUNT(*) FROM clipboard_entries WHERE is_pinned = 0",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        let step2: Vec<(String, Option<String>, Option<String>)> = if count_after > max_entries {
            let to_delete = count_after - max_entries;
            // 直接查最旧的 to_delete 条，避免 NOT IN (subquery of 10000 IDs)
            let mut stmt = tx
                .prepare(
                    "SELECT id, image_path, thumbnail_path FROM clipboard_entries
                     WHERE is_pinned = 0
                     ORDER BY created_at ASC, id ASC LIMIT ?1",
                )
                .map_err(|e| e.to_string())?;
            let rows: Vec<_> = stmt
                .query_map(params![to_delete], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            rows
        } else {
            vec![]
        };

        if !step2.is_empty() {
            let ids: Vec<&str> = step2.iter().map(|(id, _, _)| id.as_str()).collect();
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            tx.execute(
                &format!(
                    "DELETE FROM clipboard_entries WHERE id IN ({})",
                    placeholders
                ),
                rusqlite::params_from_iter(ids.iter().copied()),
            )
            .map_err(|e| e.to_string())?;
        }

        tx.commit().map_err(|e| e.to_string())?;

        let all: Vec<_> = step1.into_iter().chain(step2).collect();
        if all.is_empty() {
            return Ok((vec![], vec![]));
        }
        let ids = all.iter().map(|(id, _, _)| id.clone()).collect();
        let paths = all
            .iter()
            .flat_map(|(_, ip, tp)| [ip.clone(), tp.clone()])
            .flatten()
            .collect();
        Ok((ids, paths))
    }

    pub fn prune_after_insert(
        &self,
        window_start: i64,
        max_entries: u32,
        inserted_id: &str,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        let step1: Vec<(String, Option<String>, Option<String>)> = if window_start > 0 {
            let mut stmt = tx
                .prepare(
                    "SELECT id, image_path, thumbnail_path FROM clipboard_entries
                     WHERE is_pinned = 0 AND created_at < ?1 AND id <> ?2",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![window_start, inserted_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            rows
        } else {
            vec![]
        };

        if !step1.is_empty() {
            tx.execute(
                "DELETE FROM clipboard_entries
                 WHERE is_pinned = 0 AND created_at < ?1 AND id <> ?2",
                params![window_start, inserted_id],
            )
            .map_err(|e| e.to_string())?;
        }

        let count_after: u32 = tx
            .query_row(
                "SELECT COUNT(*) FROM clipboard_entries WHERE is_pinned = 0",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        let step2: Vec<(String, Option<String>, Option<String>)> = if count_after > max_entries {
            let to_delete = count_after - max_entries;
            let mut stmt = tx
                .prepare(
                    "SELECT id, image_path, thumbnail_path FROM clipboard_entries
                     WHERE is_pinned = 0 AND id <> ?1
                     ORDER BY created_at ASC, id ASC LIMIT ?2",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![inserted_id, to_delete], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            rows
        } else {
            vec![]
        };

        if !step2.is_empty() {
            let ids: Vec<&str> = step2.iter().map(|(id, _, _)| id.as_str()).collect();
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            tx.execute(
                &format!(
                    "DELETE FROM clipboard_entries WHERE id IN ({})",
                    placeholders
                ),
                rusqlite::params_from_iter(ids.iter().copied()),
            )
            .map_err(|e| e.to_string())?;
        }

        tx.commit().map_err(|e| e.to_string())?;

        let all: Vec<_> = step1.into_iter().chain(step2).collect();
        let ids = all.iter().map(|(id, _, _)| id.clone()).collect();
        let paths = all
            .iter()
            .flat_map(|(_, ip, tp)| [ip.clone(), tp.clone()])
            .flatten()
            .collect();
        Ok((ids, paths))
    }
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<ClipboardEntry> {
    Ok(ClipboardEntry {
        id: row.get(0)?,
        content_type: row.get(1)?,
        content: row.get(2)?,
        canonical_search_text: row.get(3)?,
        tags: Vec::new(),
        created_at: row.get(4)?,
        is_pinned: row.get::<_, i32>(5)? != 0,
        source_app: row.get::<_, String>(6).unwrap_or_default(),
        image_path: row.get(7)?,
        thumbnail_path: row.get(8)?,
    })
}
