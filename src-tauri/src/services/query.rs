use std::path::Path;

use crate::constants::DISPLAY_CONTENT_CHARS;
use crate::db::Database;
use crate::models::{ClipboardEntriesQuery, ClipboardEntry};
use crate::utils::string::{path_to_url_str, truncate_chars};

/// 截断文本 + 将图片相对路径转为完整磁盘路径。
pub fn post_process_entry(entry: &mut ClipboardEntry, data_dir: &Path) {
    if entry.content_type == "text" {
        entry.content = truncate_chars(&entry.content, DISPLAY_CONTENT_CHARS);
    } else if entry.content_type == "image" {
        entry.image_path = entry
            .image_path
            .as_deref()
            .map(|p| path_to_url_str(&data_dir.join(p)));
        entry.thumbnail_path = entry
            .thumbnail_path
            .as_deref()
            .map(|p| path_to_url_str(&data_dir.join(p)));
    }
}

/// 单遍处理多个条目，供查询接口复用。
fn post_process(entries: &mut [ClipboardEntry], data_dir: &Path) {
    for entry in entries.iter_mut() {
        post_process_entry(entry, data_dir);
    }
}

/// 返回全部置顶条目（始终全量，不分页）。
pub fn get_pinned_entries(db: &Database, data_dir: &Path) -> Result<Vec<ClipboardEntry>, String> {
    let mut entries = db.get_pinned()?;
    post_process(&mut entries, data_dir);
    Ok(entries)
}

/// 非置顶条目分页（复合游标：cursor_ts + cursor_id）。
pub fn get_normal_page(
    db: &Database,
    data_dir: &Path,
    query: &ClipboardEntriesQuery,
    window_start: i64,
) -> Result<Vec<ClipboardEntry>, String> {
    let mut entries = db.get_normal_page(query, window_start)?;
    post_process(&mut entries, data_dir);
    Ok(entries)
}

/// 返回当前可见视图中指定月份内有记录的日期列表（YYYY-MM-DD 格式）。
pub fn get_active_dates(
    db: &Database,
    year_month: &str,
    window_start: i64,
) -> Result<Vec<String>, String> {
    db.get_active_dates_in_month(year_month, window_start)
}

/// 返回当前可见视图中最早有记录的年月（YYYY-MM 格式）。
pub fn get_earliest_month(db: &Database, window_start: i64) -> Result<Option<String>, String> {
    db.get_earliest_month(window_start)
}
