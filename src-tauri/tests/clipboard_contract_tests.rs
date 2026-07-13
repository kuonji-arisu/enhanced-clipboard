use enhanced_clipboard_lib::clipboard::{ClipboardChanged, ClipboardListPage, ClipboardPreview};
use serde_json::Value;

#[test]
fn shared_clipboard_page_fixture_matches_the_rust_wire_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/contracts/clipboard_page.json")).unwrap();

    let event: ClipboardChanged = serde_json::from_value(fixture["event"].clone()).unwrap();
    let page: ClipboardListPage = serde_json::from_value(fixture["page"].clone()).unwrap();

    assert_eq!(event.revision, page.revision);
    assert_eq!(page.pinned_count, 1);
    assert_eq!(page.items.len(), 2);
    assert!(page.items[0].is_pinned);
    assert!(matches!(
        page.items[1].preview,
        ClipboardPreview::Image { src: Some(_) }
    ));

    let serialized = serde_json::to_value(page).unwrap();
    assert!(serialized.get("next_cursor").is_some());
    assert!(serialized.get("pinned_count").is_some());
    for item in serialized["items"].as_array().unwrap() {
        assert!(item.get("visible_until").is_some());
        assert!(item.get("original_path").is_none());
        assert!(item.get("preview_path").is_none());
        assert!(item.get("status").is_none());
    }
}
