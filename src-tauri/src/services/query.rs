use std::path::Path;

use crate::db::Database;
use crate::models::{ClipboardEntriesQuery, ClipboardListItem};
use crate::services::entry_tags::attach_tags;
use crate::services::projection::{project_entries_to_list_items, project_entry_to_list_item};

/// 返回命中的置顶条目（不分页）。
pub fn get_pinned_list_items(
    db: &Database,
    data_dir: &Path,
    query: &ClipboardEntriesQuery,
) -> Result<Vec<ClipboardListItem>, String> {
    let mut entries = db.get_pinned(query)?;
    attach_tags(db, &mut entries)?;
    Ok(project_entries_to_list_items(
        &entries,
        data_dir,
        query.text(),
    ))
}

/// 非置顶条目分页（复合游标：cursor_ts + cursor_id）。
pub fn get_normal_list_page(
    db: &Database,
    data_dir: &Path,
    query: &ClipboardEntriesQuery,
    window_start: i64,
) -> Result<Vec<ClipboardListItem>, String> {
    let mut entries = db.get_normal_page(query, window_start)?;
    attach_tags(db, &mut entries)?;
    Ok(project_entries_to_list_items(
        &entries,
        data_dir,
        query.text(),
    ))
}

/// Return a single list item as projected for the current snapshot query.
pub fn get_list_item_by_id(
    db: &Database,
    data_dir: &Path,
    id: &str,
    query: &ClipboardEntriesQuery,
    window_start: i64,
) -> Result<Option<ClipboardListItem>, String> {
    let Some(mut entry) = db.get_entry_by_id_for_query(id, query, window_start)? else {
        return Ok(None);
    };
    attach_tags(db, std::slice::from_mut(&mut entry))?;
    Ok(Some(project_entry_to_list_item(
        &entry,
        data_dir,
        query.text(),
    )))
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
