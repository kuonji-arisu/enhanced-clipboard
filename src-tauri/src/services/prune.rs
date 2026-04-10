use std::path::Path;

use chrono::Utc;
use log::info;
use tauri::{AppHandle, Emitter};

use crate::constants::EVENT_ENTRIES_REMOVED;
use crate::db::Database;

/// 仅当非置顶数量超出此缓冲时才执行数量截断，避免每次插入都触发 prune。
const PRUNE_BUFFER: u32 = 50;

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
/// 仅当有过期条目 OR 非置顶数量超出 max_history + PRUNE_BUFFER 时才执行。
/// 发送 `entries_removed` 事件，异步删除孤立图片文件。
pub fn prune(
    app: &AppHandle,
    db: &Database,
    data_dir: &Path,
    expiry_seconds: i64,
    max_history: u32,
) -> Result<(), String> {
    let ws = window_start(expiry_seconds);

    // 没有 TTL 且数量未超限 → 无需清理
    let count = db.count_normal()?;
    if ws == 0 && count <= max_history + PRUNE_BUFFER {
        return Ok(());
    }

    let (ids, paths) = db.prune(ws, max_history)?;
    handle_removed_entries(app, data_dir, ids, paths, "settings_or_startup")
}

/// 插入前预清理：先删 TTL 过期，再在需要时为即将插入的新非置顶条目预留一个槽位。
/// 与设置变更/启动时的批量清理不同，这里不使用 PRUNE_BUFFER，避免插入后短暂超出上限。
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
