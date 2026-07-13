use clipboard_win::{formats::Unicode, Clipboard, Setter};

const CLIPBOARD_OPEN_ATTEMPTS: usize = 10;

fn with_open_clipboard<T>(
    f: impl FnOnce() -> Result<T, clipboard_win::ErrorCode>,
) -> Result<T, String> {
    let _clip = Clipboard::new_attempts(CLIPBOARD_OPEN_ATTEMPTS).map_err(|e| e.to_string())?;
    f().map_err(|e| e.to_string())
}

pub fn write_text_to_clipboard(text: &str) -> Result<(), String> {
    with_open_clipboard(|| Unicode.write_clipboard(&text))
}
