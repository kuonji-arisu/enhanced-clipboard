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

/// 将搜索词规范为用于列表预览匹配的展示语义。
pub fn normalize_preview_query(query: &str) -> Option<String> {
    let normalized = normalize_preview_text(query);
    (!normalized.is_empty()).then_some(normalized)
}

/// 判断规范化后的列表展示文本是否命中搜索词。
pub fn normalized_preview_text_matches_query(text: &str, query: &str) -> bool {
    let Some(query) = normalize_preview_query(query) else {
        return false;
    };
    find_first_match_char_range(text, &query).is_some()
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

/// 在总预算内围绕首个命中生成前置命中的文本片段。
/// - 优先使用原样匹配
/// - 找不到时使用小写化后的宽松匹配
/// - 仍找不到时回退到前缀截断
pub fn excerpt_around_first_match(text: &str, query: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }

    let Some(query) = normalize_preview_query(query) else {
        return truncate_chars(&normalize_preview_text(text), max);
    };

    let mut boundaries: Vec<usize> = text.char_indices().map(|(idx, _)| idx).collect();
    boundaries.push(text.len());
    let total_chars = boundaries.len().saturating_sub(1);

    if total_chars <= max {
        return normalize_preview_text(text);
    }

    let Some((match_start, match_end)) = find_first_match_char_range(text, &query) else {
        return truncate_chars(&normalize_preview_text(text), max);
    };

    let match_len = match_end.saturating_sub(match_start);
    if match_len == 0 {
        return truncate_chars(&normalize_preview_text(text), max);
    }

    let available_context = max.saturating_sub(match_len);
    let target_before = available_context / 4;
    let mut before = target_before.min(match_start);
    let after_available = total_chars.saturating_sub(match_end);
    let after = (available_context - before).min(after_available);

    if before + after < available_context {
        let remaining = available_context - before - after;
        let extra_before = remaining.min(match_start - before);
        before += extra_before;
    }

    let start = match_start - before;
    let end = (match_end + after).min(total_chars);

    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    let body = normalize_preview_text(&text[boundaries[start]..boundaries[end]]);
    snippet.push_str(&body);
    if end < total_chars {
        snippet.push('…');
    }
    snippet
}

fn find_first_match_char_range(text: &str, query: &str) -> Option<(usize, usize)> {
    find_case_sensitive_char_range(text, query)
        .or_else(|| find_lowercase_fallback_char_range(text, query))
}

fn find_case_sensitive_char_range(text: &str, query: &str) -> Option<(usize, usize)> {
    let byte_start = text.find(query)?;
    let start = text[..byte_start].chars().count();
    let len = query.chars().count();
    Some((start, start + len))
}

fn find_lowercase_fallback_char_range(text: &str, query: &str) -> Option<(usize, usize)> {
    let mut lowered_text = String::new();
    let mut lowered_to_original_char = Vec::new();

    for (original_char_idx, ch) in text.chars().enumerate() {
        for lower in ch.to_lowercase() {
            lowered_text.push(lower);
            lowered_to_original_char.push(original_char_idx);
        }
    }

    let lowered_query: String = query.chars().flat_map(|ch| ch.to_lowercase()).collect();
    if lowered_query.is_empty() {
        return None;
    }

    let lowered_byte_start = lowered_text.find(&lowered_query)?;
    let lowered_char_start = lowered_text[..lowered_byte_start].chars().count();
    let lowered_char_len = lowered_query.chars().count();
    let lowered_char_end = lowered_char_start + lowered_char_len;

    let original_start = *lowered_to_original_char.get(lowered_char_start)?;
    let original_end = lowered_to_original_char
        .get(lowered_char_end.saturating_sub(1))
        .map(|idx| idx + 1)?;

    Some((original_start, original_end))
}

#[cfg(test)]
mod tests {
    use super::{
        excerpt_around_first_match, normalize_preview_query, normalize_preview_text,
        normalized_preview_text_matches_query, truncate_chars,
    };

    #[test]
    fn normalize_preview_text_compacts_all_whitespace() {
        assert_eq!(
            normalize_preview_text("alpha\r\nbeta\ngamma\rdelta\tepsilon   zeta"),
            "alpha beta gamma delta epsilon zeta"
        );
    }

    #[test]
    fn normalize_preview_text_keeps_non_preview_unicode_whitespace() {
        assert_eq!(normalize_preview_text("alpha\u{00A0}beta"), "alpha\u{00A0}beta");
    }

    #[test]
    fn normalize_preview_query_compacts_whitespace_and_trims() {
        assert_eq!(
            normalize_preview_query("  alpha\r\n\tbeta   gamma  "),
            Some("alpha beta gamma".to_string())
        );
    }

    #[test]
    fn excerpt_front_loads_context_for_cjk_match() {
        let text = "甲乙丙丁戊己庚辛壬癸";
        assert_eq!(
            excerpt_around_first_match(text, "戊", 5),
            "…丁戊己庚辛…"
        );
    }

    #[test]
    fn excerpt_falls_back_to_case_insensitive_match() {
        let text = "Alpha Beta Gamma";
        assert_eq!(
            excerpt_around_first_match(text, "beta", 10),
            "…Beta Gamm…"
        );
    }

    #[test]
    fn excerpt_handles_emoji_without_breaking_boundaries() {
        let text = "a😀bcdef";
        assert_eq!(
            excerpt_around_first_match(text, "😀b", 4),
            "…😀bcd…"
        );
    }

    #[test]
    fn excerpt_falls_back_to_truncate_when_query_not_found() {
        assert_eq!(
            excerpt_around_first_match("abcdefg", "xyz", 4),
            truncate_chars("abcdefg", 4)
        );
    }

    #[test]
    fn excerpt_returns_full_text_when_within_budget() {
        assert_eq!(excerpt_around_first_match("hello", "ell", 10), "hello");
    }

    #[test]
    fn excerpt_compacts_multiline_whitespace_so_match_stays_visible() {
        let text = "helper 最合适，继续沿用 truncate_chars() 的思路，按字符预算处理\n\n2. “左右各 N 个字符”最好改成“总预算固定”";
        let excerpt = excerpt_around_first_match(text, "左", 50);
        assert!(excerpt.contains('左'));
        assert!(!excerpt.contains('\n'));
    }

    #[test]
    fn excerpt_matches_normalized_query_whitespace() {
        let text = "foo bar baz";
        assert_eq!(
            excerpt_around_first_match(text, " foo\t bar ", 7),
            "foo bar…"
        );
    }

    #[test]
    fn normalized_preview_text_matches_query_with_case_and_whitespace_fallback() {
        assert!(normalized_preview_text_matches_query(
            "Alpha beta gamma",
            " alpha\tBETA "
        ));
    }
}
