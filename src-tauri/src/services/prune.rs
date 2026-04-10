use std::path::Path;

use chrono::Utc;
use log::info;
use tauri::{AppHandle, Emitter};

use crate::constants::EVENT_ENTRIES_REMOVED;
use crate::db::Database;

pub fn handle_removed_entries(
    app: &AppHandle,
    data_dir: &Path,
    ids: Vec<String>,
    paths: Vec<String>,
    reason: &str,
) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }

    info!(
        "Pruned entries: count={}, assets={}, reason={}",
        ids.len(),
        paths.len(),
        reason
    );

    let _ = app.emit(EVENT_ENTRIES_REMOVED, &ids);
    if !paths.is_empty() {
        let dir = data_dir.to_path_buf();
        std::thread::spawn(move || {
            for p in paths {
                let _ = std::fs::remove_file(dir.join(p));
            }
        });
    }
    Ok(())
}

/// 计算时间窗口起点（epoch 秒）。返回 0 表示不限。
pub fn window_start(expiry_seconds: i64) -> i64 {
    if expiry_seconds <= 0 {
        0
    } else {
        Utc::now().timestamp() - expiry_seconds
    }
}

/// 清理存储：保留所有置顶 + 窗口内最多 max_history 条非置顶。
/// 仅当没有 TTL 且非置顶数量未超限时直接跳过；其余情况交由 DB 按 retention 规则清理。
/// 发送 `entries_removed` 事件，异步删除孤立图片文件。
pub fn prune(
    app: &AppHandle,
    db: &Database,
    data_dir: &Path,
    expiry_seconds: i64,
    max_history: u32,
    reason: &str,
) -> Result<(), String> {
    let ws = window_start(expiry_seconds);

    // 没有 TTL 且数量未超限 → 无需清理
    let count = db.count_normal()?;
    if ws == 0 && count <= max_history {
        return Ok(());
    }

    let (ids, paths) = db.prune(ws, max_history)?;
    handle_removed_entries(app, data_dir, ids, paths, reason)
}

/// 插入前预清理：先删 TTL 过期，再在需要时为即将插入的新非置顶条目预留一个槽位。
/// 这里保留预清理，而不是复用插入后的通用 prune，
/// 是为了避免同一时间戳下新插入条目被立即裁掉。
pub fn prepare_for_insert(
    app: &AppHandle,
    db: &Database,
    data_dir: &Path,
    expiry_seconds: i64,
    max_history: u32,
) -> Result<(), String> {
    let ws = window_start(expiry_seconds);
    let count = db.count_normal()?;

    // 没有 TTL 且仍有剩余容量时，无需为本次插入做额外清理。
    if ws == 0 && count < max_history {
        return Ok(());
    }

    let reserve_slot_max = max_history.saturating_sub(1);
    let (ids, paths) = db.prune(ws, reserve_slot_max)?;
    handle_removed_entries(app, data_dir, ids, paths, "before_insert")
}
