use std::path::Path;

/// 纯文本处理工具函数。

/// 将磁盘路径转换为前端可用的 URL 字符串（统一正斜杠）。
pub(crate) fn path_to_url_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// 截取文本前 `max` 个字符，超出时末尾加省略号。
pub fn truncate_chars(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{}…", truncated)
    } else {
        truncated
    }
}
