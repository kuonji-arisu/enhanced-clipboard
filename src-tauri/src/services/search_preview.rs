use crate::constants::{DISPLAY_CONTENT_CHARS, SEARCH_WINDOW_CHARS};
use crate::models::{ClipboardPreview, ClipboardTextPreviewMode, TextRange};
use crate::utils::string::{normalize_preview_text, truncate_chars};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchableText {
    display: String,
    canonical: String,
    canonical_to_display: Vec<usize>,
}

impl SearchableText {
    fn new(raw: &str) -> Self {
        let display = normalize_preview_text(raw);
        let (canonical, canonical_to_display) = lower_with_mapping(&display);
        Self {
            display,
            canonical,
            canonical_to_display,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalQuery {
    canonical: String,
}

impl CanonicalQuery {
    fn new(raw: &str) -> Option<Self> {
        let normalized = normalize_preview_text(raw);
        (!normalized.is_empty()).then(|| Self {
            canonical: normalized
                .chars()
                .flat_map(|ch| ch.to_lowercase())
                .collect(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchMatchPlan {
    canonical_hit_ranges: Vec<TextRange>,
}

impl SearchMatchPlan {
    fn build(text: &SearchableText, query: &CanonicalQuery) -> Self {
        Self {
            canonical_hit_ranges: find_canonical_match_ranges(&text.canonical, &query.canonical),
        }
    }

    fn matched(&self) -> bool {
        !self.canonical_hit_ranges.is_empty()
    }

    fn display_hit_ranges(&self, text: &SearchableText) -> Vec<TextRange> {
        self.canonical_hit_ranges
            .iter()
            .filter_map(|range| canonical_range_to_display_range(&text.canonical_to_display, range))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnippetWindow {
    start: usize,
    end: usize,
    text: String,
    prefix_len: usize,
}

pub fn build_canonical_search_text(text: &str) -> String {
    SearchableText::new(text).canonical
}

pub fn canonicalize_query_text(query: &str) -> Option<String> {
    CanonicalQuery::new(query).map(|query| query.canonical)
}

pub fn build_text_preview(text: &str, query_text: Option<&str>) -> ClipboardPreview {
    let searchable = SearchableText::new(text);
    match query_text.and_then(CanonicalQuery::new) {
        Some(query) => build_search_preview(&searchable, &query),
        None => build_prefix_preview(&searchable),
    }
}

fn build_prefix_preview(text: &SearchableText) -> ClipboardPreview {
    ClipboardPreview::Text {
        mode: ClipboardTextPreviewMode::Prefix,
        text: truncate_chars(&text.display, DISPLAY_CONTENT_CHARS),
        highlight_ranges: Vec::new(),
    }
}

fn build_search_preview(text: &SearchableText, query: &CanonicalQuery) -> ClipboardPreview {
    let match_plan = SearchMatchPlan::build(text, query);
    if !match_plan.matched() {
        return ClipboardPreview::Text {
            mode: ClipboardTextPreviewMode::SearchSnippet,
            text: truncate_chars(&text.display, SEARCH_WINDOW_CHARS),
            highlight_ranges: Vec::new(),
        };
    }

    let display_hit_ranges = match_plan.display_hit_ranges(text);
    let window = build_snippet_window(
        &text.display,
        display_hit_ranges
            .first()
            .expect("matched() guaranteed at least one display hit"),
        SEARCH_WINDOW_CHARS,
    );
    let highlight_ranges = display_hit_ranges
        .into_iter()
        .filter_map(|range| translate_range_to_window(&range, &window))
        .collect();

    ClipboardPreview::Text {
        mode: ClipboardTextPreviewMode::SearchSnippet,
        text: window.text,
        highlight_ranges,
    }
}

fn lower_with_mapping(text: &str) -> (String, Vec<usize>) {
    let mut canonical = String::new();
    let mut canonical_to_display = Vec::new();

    for (display_idx, ch) in text.chars().enumerate() {
        for lower in ch.to_lowercase() {
            canonical.push(lower);
            canonical_to_display.push(display_idx);
        }
    }

    (canonical, canonical_to_display)
}

fn find_canonical_match_ranges(text: &str, query: &str) -> Vec<TextRange> {
    let text_chars: Vec<char> = text.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();
    if query_chars.is_empty() || query_chars.len() > text_chars.len() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut cursor = 0;

    while cursor + query_chars.len() <= text_chars.len() {
        if text_chars[cursor..cursor + query_chars.len()] == query_chars[..] {
            let end = cursor + query_chars.len();
            ranges.push(TextRange { start: cursor, end });
            cursor = end;
        } else {
            cursor += 1;
        }
    }

    ranges
}

fn canonical_range_to_display_range(mapping: &[usize], range: &TextRange) -> Option<TextRange> {
    if range.start >= range.end {
        return None;
    }

    let start = *mapping.get(range.start)?;
    let end = mapping
        .get(range.end.saturating_sub(1))
        .map(|display_idx| display_idx + 1)?;
    Some(TextRange { start, end })
}

fn build_snippet_window(text: &str, anchor: &TextRange, max: usize) -> SnippetWindow {
    let chars: Vec<char> = text.chars().collect();
    let total_chars = chars.len();

    if total_chars <= max {
        return SnippetWindow {
            start: 0,
            end: total_chars,
            text: text.to_string(),
            prefix_len: 0,
        };
    }

    let match_len = anchor.end.saturating_sub(anchor.start);
    let available_context = max.saturating_sub(match_len);
    let target_before = available_context / 4;
    let mut before = target_before.min(anchor.start);
    let after_available = total_chars.saturating_sub(anchor.end);
    let after = (available_context - before).min(after_available);

    if before + after < available_context {
        let remaining = available_context - before - after;
        let extra_before = remaining.min(anchor.start - before);
        before += extra_before;
    }

    let start = anchor.start - before;
    let end = (anchor.end + after).min(total_chars);
    let mut snippet = String::new();
    let mut prefix_len = 0;
    if start > 0 {
        snippet.push('…');
        prefix_len = 1;
    }
    snippet.extend(chars[start..end].iter());
    if end < total_chars {
        snippet.push('…');
    }

    SnippetWindow {
        start,
        end,
        text: snippet,
        prefix_len,
    }
}

fn translate_range_to_window(range: &TextRange, window: &SnippetWindow) -> Option<TextRange> {
    let start = range.start.max(window.start);
    let end = range.end.min(window.end);
    if start >= end {
        return None;
    }

    Some(TextRange {
        start: window.prefix_len + start - window.start,
        end: window.prefix_len + end - window.start,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_canonical_search_text, build_text_preview, canonicalize_query_text, CanonicalQuery,
        SearchMatchPlan, SearchableText,
    };
    use crate::models::{ClipboardPreview, ClipboardTextPreviewMode};

    #[test]
    fn build_canonical_search_text_compacts_whitespace_and_lowercases() {
        assert_eq!(
            build_canonical_search_text("Alpha\r\nBeta\tGamma"),
            "alpha beta gamma"
        );
    }

    #[test]
    fn canonicalize_query_text_compacts_whitespace() {
        assert_eq!(
            canonicalize_query_text("  Alpha \n\t Beta  "),
            Some("alpha beta".to_string())
        );
    }

    #[test]
    fn prefix_preview_does_not_expose_raw_multiline_text() {
        let preview = build_text_preview("alpha\n\nbeta\tgamma", None);
        assert_eq!(
            preview,
            ClipboardPreview::Text {
                mode: ClipboardTextPreviewMode::Prefix,
                text: "alpha beta gamma".to_string(),
                highlight_ranges: Vec::new(),
            }
        );
    }

    #[test]
    fn search_preview_is_centered_and_reports_ranges_in_preview_text() {
        let preview = build_text_preview("zero one two three four five six", Some("four"));
        let ClipboardPreview::Text {
            mode,
            text,
            highlight_ranges,
        } = preview
        else {
            panic!("expected text preview");
        };

        assert_eq!(mode, ClipboardTextPreviewMode::SearchSnippet);
        assert!(text.contains("four"));
        assert_eq!(highlight_ranges.len(), 1);
        let range = &highlight_ranges[0];
        let matched: String = text
            .chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect();
        assert_eq!(matched, "four");
    }

    #[test]
    fn search_preview_uses_normalized_whitespace_for_membership_and_highlights() {
        let searchable = SearchableText::new("Alpha\r\nBeta\tGamma");
        let query = CanonicalQuery::new(" alpha   beta ").expect("query");
        let match_plan = SearchMatchPlan::build(&searchable, &query);
        assert!(match_plan.matched());

        let preview = build_text_preview("Alpha\r\nBeta\tGamma", Some(" alpha   beta "));
        let ClipboardPreview::Text {
            text,
            highlight_ranges,
            ..
        } = preview
        else {
            panic!("expected text preview");
        };

        assert_eq!(text, "Alpha Beta Gamma");
        assert_eq!(highlight_ranges.len(), 1);
        let matched: String = text
            .chars()
            .skip(highlight_ranges[0].start)
            .take(highlight_ranges[0].end - highlight_ranges[0].start)
            .collect();
        assert_eq!(matched, "Alpha Beta");
    }

    #[test]
    fn search_preview_ranges_are_case_insensitive() {
        let preview = build_text_preview("Alpha Beta", Some("beta"));
        let ClipboardPreview::Text {
            highlight_ranges, ..
        } = preview
        else {
            panic!("expected text preview");
        };
        assert_eq!(highlight_ranges.len(), 1);
    }

    #[test]
    fn search_preview_ranges_handle_case_folding_expansion() {
        let preview = build_text_preview("İstanbul", Some("i"));
        let ClipboardPreview::Text {
            text,
            highlight_ranges,
            ..
        } = preview
        else {
            panic!("expected text preview");
        };

        assert_eq!(highlight_ranges.len(), 1);
        assert_eq!(highlight_ranges[0].start, 0);
        assert_eq!(highlight_ranges[0].end, 1);
        let matched: String = text
            .chars()
            .skip(highlight_ranges[0].start)
            .take(highlight_ranges[0].end - highlight_ranges[0].start)
            .collect();
        assert_eq!(matched, "İ");
    }

    #[test]
    fn search_preview_reports_all_visible_hits_in_snippet() {
        let preview = build_text_preview("beta beta beta", Some("beta"));
        let ClipboardPreview::Text {
            highlight_ranges, ..
        } = preview
        else {
            panic!("expected text preview");
        };

        assert_eq!(highlight_ranges.len(), 3);
    }
}
