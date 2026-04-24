use log::{debug, info};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcut, Shortcut, ShortcutState};

/// 注销所有已注册的快捷键（录制期间调用）。
pub fn unregister_hotkey<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    info!("Unregistering all global hotkeys");
    app.try_state::<GlobalShortcut<R>>()
        .ok_or_else(|| "global shortcut manager unavailable".to_string())?
        .unregister_all()
        .map_err(|e| e.to_string())
}

/// 注销所有已注册的快捷键并重新注册指定热键。
/// 若 `hotkey` 为空则仅注销，不报错。
pub fn register_hotkey<R: Runtime>(app: &AppHandle<R>, hotkey: &str) -> Result<(), String> {
    let gs = app
        .try_state::<GlobalShortcut<R>>()
        .ok_or_else(|| "global shortcut manager unavailable".to_string())?;
    gs.unregister_all().map_err(|e| e.to_string())?;

    if hotkey.is_empty() {
        info!("Global hotkey disabled");
        return Ok(());
    }

    gs.on_shortcut(hotkey, |app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            debug!("Global hotkey pressed");
            crate::utils::window::toggle_main_window(app);
        }
    })
    .map_err(|e| e.to_string())?;
    info!("Registered global hotkey: {}", hotkey);
    Ok(())
}

pub fn validate_hotkey(hotkey: &str) -> Result<(), String> {
    if hotkey.is_empty() {
        return Ok(());
    }

    hotkey
        .parse::<Shortcut>()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
