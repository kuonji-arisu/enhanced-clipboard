use serde_json::Value;

use crate::db::Database;
use crate::models::ClipboardEntry;

pub const ENTRY_ATTR_TYPE_TAG: &str = "tag";

const TAG_URL: &str = "url";
const TAG_EMAIL: &str = "email";
const TAG_JSON: &str = "json";

pub fn detect_tags_for_text(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if looks_like_json(trimmed) {
        return vec![TAG_JSON.to_string()];
    }
    if looks_like_url(trimmed) {
        return vec![TAG_URL.to_string()];
    }
    if looks_like_email(trimmed) {
        return vec![TAG_EMAIL.to_string()];
    }

    Vec::new()
}

pub fn sort_tags(tags: &mut Vec<String>) {
    tags.sort_by(|a, b| tag_priority(a).cmp(&tag_priority(b)).then_with(|| a.cmp(b)));
    tags.dedup();
}

pub fn attach_tags(db: &Database, entries: &mut [ClipboardEntry]) -> Result<(), String> {
    let entry_ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
    let tags_by_id = db.get_entry_attrs_for_ids(&entry_ids, ENTRY_ATTR_TYPE_TAG)?;

    for entry in entries.iter_mut() {
        entry.tags = tags_by_id.get(&entry.id).cloned().unwrap_or_default();
        sort_tags(&mut entry.tags);
    }

    Ok(())
}

fn looks_like_url(text: &str) -> bool {
    let Some(token) = single_token_candidate(text) else {
        return false;
    };

    (token.starts_with("http://") || token.starts_with("https://"))
        && token.len() > "https://".len()
}

fn looks_like_email(text: &str) -> bool {
    let Some(token) = single_token_candidate(text) else {
        return false;
    };

    is_email_token(token)
}

fn looks_like_json(text: &str) -> bool {
    let candidate = text.trim();
    let looks_wrapped = (candidate.starts_with('{') && candidate.ends_with('}'))
        || (candidate.starts_with('[') && candidate.ends_with(']'));

    looks_wrapped && serde_json::from_str::<Value>(candidate).is_ok()
}

fn trim_inline_token(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            ',' | '.'
                | ';'
                | ':'
                | '!'
                | '?'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | '"'
                | '\''
        )
    })
}

fn single_token_candidate(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.split_whitespace().nth(1).is_some() {
        return None;
    }

    let token = trim_inline_token(trimmed);
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn is_email_token(token: &str) -> bool {
    if token.is_empty() || token.contains("://") {
        return false;
    }

    let mut parts = token.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };

    if parts.next().is_some() || local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return false;
    }

    local
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-'))
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        && domain.split('.').all(|segment| {
            !segment.is_empty() && !segment.starts_with('-') && !segment.ends_with('-')
        })
}

fn tag_priority(tag: &str) -> u8 {
    match tag {
        TAG_JSON => 0,
        TAG_URL => 1,
        TAG_EMAIL => 2,
        _ => u8::MAX,
    }
}
