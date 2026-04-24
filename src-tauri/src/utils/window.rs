use tauri::{Emitter, Manager, Runtime};

use crate::constants::{EVENT_UI_RESUME, EVENT_UI_SUSPEND, MAIN_WINDOW_LABEL};

pub trait UiLifecycleWindow<R: Runtime> {
    fn is_visible_for_lifecycle(&self) -> bool;
    fn hide_for_lifecycle(&self);
    fn emit_for_lifecycle(&self, event: &str);
}

impl<R: Runtime> UiLifecycleWindow<R> for tauri::Window<R> {
    fn is_visible_for_lifecycle(&self) -> bool {
        self.is_visible().unwrap_or(false)
    }

    fn hide_for_lifecycle(&self) {
        let _ = self.hide();
    }

    fn emit_for_lifecycle(&self, event: &str) {
        let _ = self.emit(event, ());
    }
}

impl<R: Runtime> UiLifecycleWindow<R> for tauri::WebviewWindow<R> {
    fn is_visible_for_lifecycle(&self) -> bool {
        self.is_visible().unwrap_or(false)
    }

    fn hide_for_lifecycle(&self) {
        let _ = self.hide();
    }

    fn emit_for_lifecycle(&self, event: &str) {
        let _ = self.emit(event, ());
    }
}

pub fn show_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let was_visible = window.is_visible().unwrap_or(false);
        let _ = window.show();
        let _ = window.set_focus();
        if !was_visible {
            let _ = window.emit(EVENT_UI_RESUME, ());
        }
    }
}

pub fn hide_main_window<R: Runtime, W: UiLifecycleWindow<R>>(window: &W) {
    if !window.is_visible_for_lifecycle() {
        return;
    }

    window.emit_for_lifecycle(EVENT_UI_SUSPEND);
    window.hide_for_lifecycle();
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
