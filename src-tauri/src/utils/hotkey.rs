use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use log::{debug, info};

use crate::constants::MAIN_WINDOW_LABEL;

/// 注销所有已注册的快捷键（录制期间调用）。
pub fn unregister_hotkey(app: &AppHandle) -> Result<(), String> {
    info!("Unregistering all global hotkeys");
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())
}

/// 注销所有已注册的快捷键并重新注册指定热键。
/// 若 `hotkey` 为空则仅注销，不报错。
pub fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    gs.unregister_all().map_err(|e| e.to_string())?;

    if hotkey.is_empty() {
        info!("Global hotkey disabled");
        return Ok(());
    }

    gs.on_shortcut(hotkey, |app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            debug!("Global hotkey pressed");
            if let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                if win.is_visible().unwrap_or(false) {
                    let _ = win.hide();
                } else {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        }
    })
    .map_err(|e| e.to_string())?;
    info!("Registered global hotkey: {}", hotkey);
    Ok(())
}
