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
    display_hit_ranges: Vec<TextRange>,
    snippet_anchor: Option<TextRange>,
}

impl SearchMatchPlan {
    fn build(text: &SearchableText, query: &CanonicalQuery) -> Self {
        let canonical_hit_ranges = find_canonical_match_ranges(&text.canonical, &query.canonical);
        let display_hit_ranges = canonical_hit_ranges
            .iter()
            .filter_map(|range| canonical_range_to_display_range(&text.canonical_to_display, range))
            .collect::<Vec<_>>();

        Self {
            canonical_hit_ranges,
            snippet_anchor: display_hit_ranges.first().cloned(),
            display_hit_ranges,
        }
    }

    fn matched(&self) -> bool {
        !self.canonical_hit_ranges.is_empty()
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
    build_preview_from_match_plan(text, &match_plan)
}

fn build_preview_from_match_plan(
    text: &SearchableText,
    match_plan: &SearchMatchPlan,
) -> ClipboardPreview {
    if !match_plan.matched() {
        return ClipboardPreview::Text {
            mode: ClipboardTextPreviewMode::SearchSnippet,
            text: truncate_chars(&text.display, SEARCH_WINDOW_CHARS),
            highlight_ranges: Vec::new(),
        };
    }

    let Some(anchor) = match_plan.snippet_anchor.as_ref() else {
        return ClipboardPreview::Text {
            mode: ClipboardTextPreviewMode::SearchSnippet,
            text: truncate_chars(&text.display, SEARCH_WINDOW_CHARS),
            highlight_ranges: Vec::new(),
        };
    };

    let window = build_snippet_window(&text.display, anchor, SEARCH_WINDOW_CHARS);
    let highlight_ranges = match_plan
        .display_hit_ranges
        .iter()
        .filter_map(|range| translate_range_to_window(range, &window))
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
