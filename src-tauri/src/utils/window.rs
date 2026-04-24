use tauri::{Emitter, Manager, Runtime};

use crate::constants::{EVENT_UI_RESUME, EVENT_UI_SUSPEND, MAIN_WINDOW_LABEL};

pub fn show_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit(EVENT_UI_RESUME, ());
    }
}

pub fn hide_main_window<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let _ = window.emit(EVENT_UI_SUSPEND, ());
    let _ = window.hide();
}

pub fn toggle_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if window.is_visible().unwrap_or(false) {
            hide_main_window(&window);
        } else {
            show_main_window(app);
        }
    }
}
