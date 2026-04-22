use crate::constants::{DISPLAY_CONTENT_CHARS, SEARCH_WINDOW_CHARS};
use crate::models::{ClipboardPreviewKind, TextRange};
use crate::utils::string::{excerpt_around_first_match, normalize_preview_text, truncate_chars};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPreview {
    pub text: String,
    pub kind: ClipboardPreviewKind,
    pub match_ranges: Vec<TextRange>,
}

pub fn build_text_preview(text: &str, search_text: Option<&str>) -> TextPreview {
    match search_text.and_then(normalize_query) {
        Some(query) => {
            let preview_text = excerpt_around_first_match(text, query, SEARCH_WINDOW_CHARS);
            TextPreview {
                match_ranges: find_match_ranges(&preview_text, query),
                text: preview_text,
                kind: ClipboardPreviewKind::SearchSnippet,
            }
        }
        None => TextPreview {
            text: truncate_chars(&normalize_preview_text(text), DISPLAY_CONTENT_CHARS),
            kind: ClipboardPreviewKind::Prefix,
            match_ranges: Vec::new(),
        },
    }
}

fn normalize_query(query: &str) -> Option<&str> {
    let query = query.trim();
    (!query.is_empty()).then_some(query)
}

fn find_match_ranges(text: &str, query: &str) -> Vec<TextRange> {
    let chars: Vec<char> = text.chars().collect();
    let lowered_query: String = query.chars().flat_map(|ch| ch.to_lowercase()).collect();
    if lowered_query.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut cursor = 0;

    while cursor < chars.len() {
        let lowered_suffix: String = chars[cursor..]
            .iter()
            .flat_map(|ch| ch.to_lowercase())
            .collect();

        if !lowered_suffix.starts_with(&lowered_query) {
            cursor += 1;
            continue;
        }

        let mut lowered_match = String::new();
        let mut end = cursor;
        while end < chars.len() && lowered_match.len() < lowered_query.len() {
            lowered_match.extend(chars[end].to_lowercase());
            end += 1;
        }

        ranges.push(TextRange { start: cursor, end });
        cursor = end;
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::build_text_preview;
    use crate::models::ClipboardPreviewKind;

    #[test]
    fn prefix_preview_does_not_expose_raw_multiline_text() {
        let preview = build_text_preview("alpha\n\nbeta\tgamma", None);
        assert_eq!(preview.text, "alpha beta gamma");
        assert_eq!(preview.kind, ClipboardPreviewKind::Prefix);
        assert!(preview.match_ranges.is_empty());
    }

    #[test]
    fn search_preview_is_centered_and_reports_ranges_in_preview_text() {
        let preview = build_text_preview("zero one two three four five six", Some("four"));
        assert_eq!(preview.kind, ClipboardPreviewKind::SearchSnippet);
        assert!(preview.text.contains("four"));
        assert_eq!(preview.match_ranges.len(), 1);
        let range = &preview.match_ranges[0];
        let matched: String = preview
            .text
            .chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect();
        assert_eq!(matched, "four");
    }

    #[test]
    fn search_preview_ranges_are_case_insensitive() {
        let preview = build_text_preview("Alpha Beta", Some("beta"));
        assert_eq!(preview.match_ranges.len(), 1);
    }

    #[test]
    fn search_preview_ranges_handle_case_folding_expansion() {
        let preview = build_text_preview("İstanbul", Some("i"));
        assert_eq!(preview.match_ranges.len(), 1);
        assert_eq!(preview.match_ranges[0].start, 0);
        assert_eq!(preview.match_ranges[0].end, 1);
    }
}
