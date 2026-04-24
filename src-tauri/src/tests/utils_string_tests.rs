use crate::utils::string::{normalize_preview_text, truncate_chars};

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

#[test]
fn truncate_chars_adds_ellipsis_only_when_needed() {
    assert_eq!(truncate_chars("alpha", 10), "alpha");
    assert_eq!(truncate_chars("alphabet", 5), "alpha…");
}
