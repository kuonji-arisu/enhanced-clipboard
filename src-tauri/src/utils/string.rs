use std::path::Path;

/// 纯文本处理工具函数。

/// 将磁盘路径转换为前端可用的 URL 字符串（统一正斜杠）。
pub(crate) fn path_to_url_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// 将普通空格、制表和换行统一规范为单个空格，供列表预览展示使用。
pub fn normalize_preview_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_was_preview_space = true;

    for ch in text.chars() {
        let is_preview_space = matches!(ch, ' ' | '\r' | '\n' | '\t');
        if is_preview_space {
            if !previous_was_preview_space {
                normalized.push(' ');
            }
        } else {
            normalized.push(ch);
        }
        previous_was_preview_space = is_preview_space;
    }

    if normalized.ends_with(' ') {
        normalized.pop();
    }

    normalized
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

#[cfg(test)]
mod tests {
    use super::normalize_preview_text;

    #[test]
    fn normalize_preview_text_compacts_all_whitespace() {
        assert_eq!(
            normalize_preview_text("alpha\r\nbeta\ngamma\rdelta\tepsilon   zeta"),
            "alpha beta gamma delta epsilon zeta"
        );
    }

    #[test]
    fn normalize_preview_text_keeps_non_preview_unicode_whitespace() {
        assert_eq!(
            normalize_preview_text("alpha\u{00A0}beta"),
            "alpha\u{00A0}beta"
        );
    }
}
