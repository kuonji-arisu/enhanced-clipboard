use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::constants::{DISPLAY_CONTENT_CHARS, PAGE_SIZE, SEARCH_WINDOW_CHARS};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ClipboardContentType {
    Text,
    Image,
}

impl ClipboardContentType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, String> {
        match value {
            "text" => Ok(Self::Text),
            "image" => Ok(Self::Image),
            _ => Err(format!("Unknown clipboard content type: {value}")),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CapturedPayload {
    Text {
        content: String,
        source_app: String,
    },
    Image {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        source_app: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardTextPreviewMode {
    Prefix,
    SearchSnippet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipboardPreview {
    Text {
        mode: ClipboardTextPreviewMode,
        text: String,
        #[serde(default)]
        highlight_ranges: Vec<TextRange>,
    },
    Image {
        /// Asset-protocol URL ready for direct use in an `<img>` element.
        src: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardListItem {
    pub id: String,
    pub content_type: ClipboardContentType,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: i64,
    pub is_pinned: bool,
    pub source_app: String,
    pub preview: ClipboardPreview,
    /// Absolute epoch second at which a non-pinned item becomes invisible.
    /// `None` means it does not expire in the current policy.
    pub visible_until: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardQueryCursor {
    pub created_at: i64,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct ClipboardEntriesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(rename = "entryType", skip_serializing_if = "Option::is_none")]
    pub entry_type: Option<ClipboardContentType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ClipboardQueryCursor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl ClipboardEntriesQuery {
    pub(crate) fn text(&self) -> Option<&str> {
        self.text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn tag(&self) -> Option<&str> {
        self.tag
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn date(&self) -> Option<&str> {
        self.date
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn normalized_limit(&self) -> u32 {
        self.limit.unwrap_or(PAGE_SIZE).clamp(1, PAGE_SIZE)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardListPage {
    pub revision: u64,
    pub items: Vec<ClipboardListItem>,
    pub next_cursor: Option<ClipboardQueryCursor>,
    /// Global pinned count, intentionally independent of the active filters.
    pub pinned_count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardChanged {
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImagePreviewRepairOutcome {
    Repaired,
    Removed,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryRecord {
    pub id: String,
    pub content_type: ClipboardContentType,
    pub content: String,
    pub created_at: i64,
    pub is_pinned: bool,
    pub source_app: String,
    pub original_rel_path: Option<String>,
    pub preview_rel_path: Option<String>,
    pub tags: Vec<String>,
}

pub(crate) fn project_entry(
    entry: EntryRecord,
    data_dir: &Path,
    query_text: Option<&str>,
    expiry_seconds: i64,
) -> ClipboardListItem {
    let preview = match entry.content_type {
        ClipboardContentType::Text => build_text_preview(&entry.content, query_text),
        ClipboardContentType::Image => ClipboardPreview::Image {
            src: entry
                .preview_rel_path
                .as_deref()
                .and_then(|path| existing_asset_url(data_dir, path)),
        },
    };
    let visible_until = if entry.is_pinned || expiry_seconds <= 0 {
        None
    } else {
        Some(entry.created_at.saturating_add(expiry_seconds))
    };

    ClipboardListItem {
        id: entry.id,
        content_type: entry.content_type,
        tags: entry.tags,
        created_at: entry.created_at,
        is_pinned: entry.is_pinned,
        source_app: entry.source_app,
        preview,
        visible_until,
    }
}

pub(crate) fn build_canonical_search_text(text: &str) -> String {
    let display = normalize_preview_text(text);
    display.chars().flat_map(char::to_lowercase).collect()
}

pub(crate) fn canonicalize_query_text(text: &str) -> Option<String> {
    let value = build_canonical_search_text(text);
    (!value.is_empty()).then_some(value)
}

fn existing_asset_url(data_dir: &Path, rel_path: &str) -> Option<String> {
    let path = super::artifacts::validate_relative_path(data_dir, rel_path)?;
    path.is_file().then(|| asset_url(&path))
}

fn asset_url(path: &Path) -> String {
    let encoded = encode_uri_component(path.to_string_lossy().as_bytes());
    if cfg!(windows) {
        format!("http://asset.localhost/{encoded}")
    } else {
        format!("asset://localhost/{encoded}")
    }
}

fn encode_uri_component(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len());
    for byte in bytes {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            encoded.push(ch);
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[derive(Debug)]
struct SearchableText {
    display: String,
    canonical: String,
    canonical_to_display: Vec<usize>,
}

impl SearchableText {
    fn new(raw: &str) -> Self {
        let display = normalize_preview_text(raw);
        let mut canonical = String::new();
        let mut canonical_to_display = Vec::new();
        for (display_index, ch) in display.chars().enumerate() {
            for lower in ch.to_lowercase() {
                canonical.push(lower);
                canonical_to_display.push(display_index);
            }
        }
        Self {
            display,
            canonical,
            canonical_to_display,
        }
    }
}

fn build_text_preview(text: &str, query_text: Option<&str>) -> ClipboardPreview {
    let text = SearchableText::new(text);
    let Some(query) = query_text.and_then(canonicalize_query_text) else {
        return ClipboardPreview::Text {
            mode: ClipboardTextPreviewMode::Prefix,
            text: truncate_chars(&text.display, DISPLAY_CONTENT_CHARS),
            highlight_ranges: Vec::new(),
        };
    };

    let canonical_ranges = find_match_ranges(&text.canonical, &query);
    let display_ranges = canonical_ranges
        .iter()
        .filter_map(|range| canonical_range_to_display(&text.canonical_to_display, range))
        .collect::<Vec<_>>();
    let Some(anchor) = display_ranges.first() else {
        return ClipboardPreview::Text {
            mode: ClipboardTextPreviewMode::SearchSnippet,
            text: truncate_chars(&text.display, SEARCH_WINDOW_CHARS),
            highlight_ranges: Vec::new(),
        };
    };

    let window = snippet_window(&text.display, anchor, SEARCH_WINDOW_CHARS);
    let highlight_ranges = display_ranges
        .iter()
        .filter_map(|range| translate_range_to_window(range, &window))
        .collect();
    ClipboardPreview::Text {
        mode: ClipboardTextPreviewMode::SearchSnippet,
        text: window.text,
        highlight_ranges,
    }
}

fn normalize_preview_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_was_space = true;
    for ch in text.chars() {
        let is_space = matches!(ch, ' ' | '\r' | '\n' | '\t');
        if is_space {
            if !previous_was_space {
                normalized.push(' ');
            }
        } else {
            normalized.push(ch);
        }
        previous_was_space = is_space;
    }
    if normalized.ends_with(' ') {
        normalized.pop();
    }
    normalized
}

fn truncate_chars(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn find_match_ranges(text: &str, query: &str) -> Vec<TextRange> {
    let text = text.chars().collect::<Vec<_>>();
    let query = query.chars().collect::<Vec<_>>();
    if query.is_empty() || query.len() > text.len() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut cursor = 0;
    while cursor + query.len() <= text.len() {
        if text[cursor..cursor + query.len()] == query[..] {
            let end = cursor + query.len();
            result.push(TextRange { start: cursor, end });
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    result
}

fn canonical_range_to_display(mapping: &[usize], range: &TextRange) -> Option<TextRange> {
    if range.start >= range.end {
        return None;
    }
    Some(TextRange {
        start: *mapping.get(range.start)?,
        end: mapping.get(range.end - 1)? + 1,
    })
}

struct SnippetWindow {
    start: usize,
    end: usize,
    text: String,
    prefix_len: usize,
}

fn snippet_window(text: &str, anchor: &TextRange, max: usize) -> SnippetWindow {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max {
        return SnippetWindow {
            start: 0,
            end: chars.len(),
            text: text.to_string(),
            prefix_len: 0,
        };
    }
    let match_len = anchor.end.saturating_sub(anchor.start);
    let context = max.saturating_sub(match_len);
    let mut before = (context / 4).min(anchor.start);
    let after = (context - before).min(chars.len().saturating_sub(anchor.end));
    if before + after < context {
        before += (context - before - after).min(anchor.start - before);
    }
    let start = anchor.start - before;
    let end = (anchor.end + after).min(chars.len());
    let mut snippet = String::new();
    let prefix_len = usize::from(start > 0);
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(chars[start..end].iter());
    if end < chars.len() {
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
    (start < end).then(|| TextRange {
        start: window.prefix_len + start - window.start,
        end: window.prefix_len + end - window.start,
    })
}
