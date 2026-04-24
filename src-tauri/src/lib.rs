mod commands;
mod constants;
mod db;
mod i18n;
mod models;
mod services;
#[cfg(test)]
mod tests;
mod utils;
mod watcher;

use log::{debug, error, info, warn};
use std::sync::{Arc, RwLock};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

use constants::{AUTOSTART_ARG, DEFAULT_LOG_LEVEL, LOG_FILE_NAME, MAIN_WINDOW_LABEL};
use db::{Database, SettingsStore};
use models::{AppInfoState, DataDir, PersistedStatePatch, RuntimeStatusState};
use watcher::ClipboardWatcher;

fn init_storage_dirs(app: &tauri::App) -> Result<std::path::PathBuf, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data_dir.join("images")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data_dir.join("thumbnails")).map_err(|e| e.to_string())?;
    Ok(data_dir)
}

fn open_or_report<T, F>(path: &str, kind: &str, open: F) -> Result<T, String>
where
    F: Fn(&str) -> Result<T, String>,
{
    open(path).map_err(|e| format!("初始化{kind}失败: {e}"))
}

fn init_clipboard_database(data_dir: &std::path::Path) -> Result<Database, String> {
    let clipboard_db_path = data_dir.join("clipboard.db").to_string_lossy().to_string();
    let clipboard_db_key = crate::utils::secure::get_or_create_clipboard_db_key().map_err(|e| {
        error!("Failed to get or create clipboard DB key: {e}");
        format!("初始化剪贴板数据库密钥失败: {e}")
    })?;

    Database::new(
        &clipboard_db_path,
        &clipboard_db_key.raw_key_hex,
        clipboard_db_key.was_created,
    )
    .map_err(|e| {
        error!("Failed to initialize clipboard database: {e}");
        format!("初始化剪贴板数据库失败: {e}")
    })
}

fn init_settings_store(data_dir: &std::path::Path) -> Result<SettingsStore, String> {
    let settings_db_path = data_dir.join("settings.db").to_string_lossy().to_string();
    open_or_report(&settings_db_path, "设置存储", SettingsStore::new).map_err(|e| {
        error!("Failed to initialize settings store: {e}");
        e
    })
}

fn manage_app_state(
    app: &mut tauri::App,
    db: Arc<Database>,
    settings_store: Arc<SettingsStore>,
    watcher: ClipboardWatcher,
    data_dir: std::path::PathBuf,
    runtime_status: Arc<RuntimeStatusState>,
    app_info: AppInfoState,
) {
    app.manage(db);
    app.manage(settings_store);
    app.manage(watcher);
    app.manage(DataDir(data_dir));
    app.manage(runtime_status);
    app.manage(app_info);
}

fn apply_window_icon(app: &tauri::App) {
    if let (Some(win), Some(icon)) = (
        app.get_webview_window(MAIN_WINDOW_LABEL),
        app.default_window_icon().cloned(),
    ) {
        let _ = win.set_icon(icon);
    }
}

fn setup_tray_menu(app: &mut tauri::App) -> Result<(), String> {
    let locale = app.state::<AppInfoState>().0.locale.clone();
    let tr = i18n::load(&locale);
    let app_title = tr.t("appTitle");
    let show_txt = tr.t("show");
    let quit_txt = tr.t("quit");
    app.manage(Arc::new(RwLock::new(tr)));

    let show_item =
        MenuItem::with_id(app, "show", &show_txt, true, None::<&str>).map_err(|e| e.to_string())?;
    let quit_item =
        MenuItem::with_id(app, "quit", &quit_txt, true, None::<&str>).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item]).map_err(|e| e.to_string())?;

    let tray_icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "Default window icon not found".to_string())?;
    let _tray = TrayIconBuilder::with_id("main_tray")
        .icon(tray_icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(&app_title)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => crate::utils::window::show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                crate::utils::window::toggle_main_window(app);
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        // 单实例：若已有进程在运行，将其窗口显示到前台，然后退出当前进程
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            crate::utils::window::show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // 自启动时传入 AUTOSTART_ARG 标记，用于检测静默启动
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = init_storage_dirs(app)?;
            crate::utils::logging::init(&data_dir.join(LOG_FILE_NAME), DEFAULT_LOG_LEVEL)
                .map_err(|e| format!("初始化日志失败: {e}"))?;
            info!("Application setup started");
            debug!("App data directory: {}", data_dir.display());

            let db = Arc::new(init_clipboard_database(&data_dir)?);
            let settings_store = Arc::new(init_settings_store(&data_dir)?);
            let app_info = AppInfoState(services::app_info::build_app_info(app.handle()));
            let runtime_status = Arc::new(RuntimeStatusState(std::sync::Mutex::new(
                services::runtime::initial_status(),
            )));

            let watcher = ClipboardWatcher::new();
            watcher.start(
                app.handle().clone(),
                db.clone(),
                settings_store.clone(),
                data_dir.clone(),
                runtime_status.clone(),
            );
            watcher.initialize_system_theme(app.handle(), &runtime_status);

            manage_app_state(
                app,
                db,
                settings_store,
                watcher,
                data_dir.clone(),
                runtime_status,
                app_info,
            );
            setup_tray_menu(app)?;
            let i18n = app.state::<Arc<RwLock<i18n::I18n>>>();
            if let Err(e) = services::settings::restore_settings_effects(
                app.handle(),
                &app.state::<Arc<Database>>(),
                &app.state::<Arc<SettingsStore>>(),
                &app.state::<ClipboardWatcher>(),
                &data_dir,
                &i18n,
            ) {
                warn!("Failed to restore settings effects: {}", e);
            }
            if let Err(e) = services::persisted_state::restore_persisted_effects(
                app.handle(),
                &app.state::<Arc<SettingsStore>>(),
            ) {
                warn!("Failed to restore persisted effects: {}", e);
            }

            let is_autostart = std::env::args().any(|a| a == AUTOSTART_ARG);
            if !is_autostart {
                crate::utils::window::show_main_window(app.handle());
            }

            apply_window_icon(app);

            info!("Application setup completed");

            Ok(())
        })
        // 拦截关闭事件：保存窗口位置，然后隐藏到托盘
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::ThemeChanged(theme) if window.label() == MAIN_WINDOW_LABEL => {
                    let app = window.app_handle();
                    let watcher = app.state::<ClipboardWatcher>();
                    let runtime_status = app.state::<Arc<RuntimeStatusState>>();
                    watcher.handle_system_theme_change(&app, runtime_status.inner(), theme.clone());
                }
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    // 保存当前窗口位置
                    if let Ok(pos) = window.outer_position() {
                        let app = window.app_handle();
                        let store = app.state::<Arc<SettingsStore>>();
                        let i18n = app.state::<Arc<RwLock<i18n::I18n>>>();
                        if let Err(e) = services::persisted_state::save_persisted(
                            app,
                            &store,
                            &i18n,
                            PersistedStatePatch {
                                window_x: Some(Some(pos.x)),
                                window_y: Some(Some(pos.y)),
                                always_on_top: None,
                            },
                        ) {
                            warn!("Failed to persist window position: {}", e);
                        }
                    }
                    api.prevent_close();
                    let app = window.app_handle();
                    if let Some(main_window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                        crate::utils::window::hide_main_window(&main_window);
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_clipboard_list_items,
            commands::get_clipboard_list_item,
            commands::copy_entry,
            commands::delete_entry,
            commands::report_image_load_failed,
            commands::toggle_pin,
            commands::clear_all,
            commands::get_active_dates,
            commands::get_earliest_month,
            commands::get_settings,
            commands::get_persisted,
            commands::get_runtime_status,
            commands::save_settings,
            commands::save_persisted,
            commands::pause_hotkey,
            commands::resume_hotkey,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
