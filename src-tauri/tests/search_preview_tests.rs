use enhanced_clipboard_lib::models::{ClipboardPreview, ClipboardTextPreviewMode, TextRange};
use enhanced_clipboard_lib::services::search_preview::{
    build_canonical_search_text, build_text_preview, canonicalize_query_text,
};

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
    assert_eq!(
        build_text_preview("alpha\n\nbeta\tgamma", None),
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
    assert_eq!(highlight_ranges, vec![TextRange { start: 0, end: 10 }]);
}

#[test]
fn search_preview_ranges_are_case_insensitive() {
    let ClipboardPreview::Text {
        highlight_ranges, ..
    } = build_text_preview("Alpha Beta", Some("beta"))
    else {
        panic!("expected text preview");
    };

    assert_eq!(highlight_ranges.len(), 1);
}

#[test]
fn search_preview_ranges_handle_case_folding_expansion() {
    let ClipboardPreview::Text {
        text,
        highlight_ranges,
        ..
    } = build_text_preview("İstanbul", Some("i"))
    else {
        panic!("expected text preview");
    };

    assert_eq!(highlight_ranges, vec![TextRange { start: 0, end: 1 }]);
    let matched: String = text
        .chars()
        .skip(highlight_ranges[0].start)
        .take(highlight_ranges[0].end - highlight_ranges[0].start)
        .collect();
    assert_eq!(matched, "İ");
}

#[test]
fn search_preview_reports_all_visible_hits_in_snippet() {
    let ClipboardPreview::Text {
        highlight_ranges, ..
    } = build_text_preview("beta beta beta", Some("beta"))
    else {
        panic!("expected text preview");
    };

    assert_eq!(highlight_ranges.len(), 3);
}
