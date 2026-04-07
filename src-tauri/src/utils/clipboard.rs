use std::path::Path;

use clipboard_win::{
    formats::{FileList, Unicode},
    Clipboard, Setter,
};

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

pub fn write_file_to_clipboard(path: &Path) -> Result<(), String> {
    let path = path.to_string_lossy().into_owned();
    with_open_clipboard(|| FileList.write_clipboard(std::slice::from_ref(&path)))
}
