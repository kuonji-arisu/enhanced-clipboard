use std::path::Path;

use crate::constants::{DISPLAY_CONTENT_CHARS, SEARCH_WINDOW_CHARS};
use crate::db::Database;
use crate::models::{ClipboardEntriesQuery, ClipboardEntry};
use crate::services::entry_tags::attach_tags;
use crate::utils::string::{
    excerpt_around_first_match, normalize_preview_text, path_to_url_str, truncate_chars,
};

pub fn shape_text_preview(text: &str, search_text: Option<&str>) -> String {
    match search_text {
        Some(query) => excerpt_around_first_match(text, query, SEARCH_WINDOW_CHARS),
        None => truncate_chars(&normalize_preview_text(text), DISPLAY_CONTENT_CHARS),
    }
}

/// 截断文本 + 将图片相对路径转为完整磁盘路径。
pub fn post_process_entry(
    entry: &mut ClipboardEntry,
    data_dir: &Path,
    search_text: Option<&str>,
) {
    if entry.content_type == "text" {
        entry.content = shape_text_preview(&entry.content, search_text);
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
fn post_process(entries: &mut [ClipboardEntry], data_dir: &Path, search_text: Option<&str>) {
    for entry in entries.iter_mut() {
        post_process_entry(entry, data_dir, search_text);
    }
}

/// 返回命中的置顶条目（不分页）。
pub fn get_pinned_entries(
    db: &Database,
    data_dir: &Path,
    query: &ClipboardEntriesQuery,
) -> Result<Vec<ClipboardEntry>, String> {
    let mut entries = db.get_pinned(query)?;
    attach_tags(db, &mut entries)?;
    post_process(&mut entries, data_dir, query.text());
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
    attach_tags(db, &mut entries)?;
    post_process(&mut entries, data_dir, query.text());
    Ok(entries)
}

/// 返回单条记录在给定查询语义下是否仍应出现在当前结果集中。
pub fn resolve_entry_for_query(
    db: &Database,
    data_dir: &Path,
    query: &ClipboardEntriesQuery,
    window_start: i64,
    id: &str,
) -> Result<Option<ClipboardEntry>, String> {
    // 成员资格判断基于当前激活视图的过滤条件，而不是某一页的 cursor 切片。
    let membership_query = ClipboardEntriesQuery {
        text: query.text.clone(),
        tag: query.tag.clone(),
        entry_type: query.entry_type(),
        date: query.date.clone(),
        cursor: None,
        limit: None,
    };

    if let Some(mut entry) = db.get_pinned_entry_by_id_for_query(id, &membership_query)? {
        attach_tags(db, std::slice::from_mut(&mut entry))?;
        post_process_entry(&mut entry, data_dir, membership_query.text());
        return Ok(Some(entry));
    }

    let Some(mut entry) =
        db.get_normal_entry_by_id_for_query(id, &membership_query, window_start)?
    else {
        return Ok(None);
    };

    attach_tags(db, std::slice::from_mut(&mut entry))?;
    post_process_entry(&mut entry, data_dir, membership_query.text());
    Ok(Some(entry))
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
