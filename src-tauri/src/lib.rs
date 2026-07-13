pub mod clipboard;
mod commands;
pub mod constants;
pub mod db;
pub mod i18n;
pub mod models;
pub mod services;
pub mod utils;
pub mod watcher;

use std::sync::{Arc, RwLock};

use clipboard::{
    ClipboardChanged, ClipboardEngine, ClipboardEngineConfig, ClipboardEngineHandle,
    ClipboardPolicy,
};
use log::{debug, error, info, warn};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

use constants::{AUTOSTART_ARG, DEFAULT_LOG_LEVEL, LOG_FILE_NAME, MAIN_WINDOW_LABEL};
use db::SettingsStore;
use models::{AppInfoState, PersistedStatePatch, RuntimeStatusState};
use watcher::{ClipboardWatcher, WatcherStartContext};

fn init_storage_dir(app: &tauri::App) -> Result<std::path::PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
    Ok(data_dir)
}

fn init_settings_store(data_dir: &std::path::Path) -> Result<SettingsStore, String> {
    let path = data_dir.join("settings.db");
    SettingsStore::new(path.to_string_lossy().as_ref()).map_err(|error| {
        error!("Failed to initialize settings store: {error}");
        format!("Failed to initialize settings store: {error}")
    })
}

fn init_clipboard_engine(
    app: &tauri::AppHandle,
    data_dir: &std::path::Path,
    settings: &SettingsStore,
) -> Result<ClipboardEngineHandle, String> {
    let saved = settings.load_runtime_app_settings()?;
    let policy = ClipboardPolicy {
        expiry_seconds: saved.expiry_seconds,
        max_history: saved.max_history,
        capture_images: saved.capture_images,
    };
    let key = crate::utils::secure::get_or_create_clipboard_db_key().map_err(|error| {
        error!("Failed to get or create clipboard database key: {error}");
        format!("Failed to initialize clipboard database key: {error}")
    })?;
    let event_app = app.clone();
    let config = ClipboardEngineConfig::new(
        data_dir.join("clipboard.db"),
        key.raw_key_hex,
        key.was_created,
        data_dir,
        policy,
        move |payload: ClipboardChanged| {
            event_app
                .emit(constants::EVENT_CLIPBOARD_CHANGED, payload)
                .map_err(|error| error.to_string())
        },
    );

    ClipboardEngine::start(config)
        .map(ClipboardEngine::into_handle)
        .map_err(|error| {
            error!("Failed to initialize clipboard engine: {error}");
            error.to_string()
        })
}

struct ManagedAppState {
    clipboard: ClipboardEngineHandle,
    settings_store: Arc<SettingsStore>,
    runtime_status: Arc<RuntimeStatusState>,
    app_info: AppInfoState,
}

fn manage_app_state(app: &mut tauri::App, state: ManagedAppState) {
    app.manage(state.clipboard);
    app.manage(state.settings_store);
    app.manage(state.runtime_status);
    app.manage(state.app_info);
}

fn apply_window_icon(app: &tauri::App) {
    if let (Some(window), Some(icon)) = (
        app.get_webview_window(MAIN_WINDOW_LABEL),
        app.default_window_icon().cloned(),
    ) {
        let _ = window.set_icon(icon);
    }
}

fn setup_tray_menu(app: &mut tauri::App) -> Result<(), String> {
    let locale = app.state::<AppInfoState>().0.locale.clone();
    let tr = i18n::load(&locale);
    let app_title = tr.t("appTitle");
    let show_text = tr.t("show");
    let quit_text = tr.t("quit");
    app.manage(Arc::new(RwLock::new(tr)));

    let show_item = MenuItem::with_id(app, "show", &show_text, true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let quit_item = MenuItem::with_id(app, "quit", &quit_text, true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let menu =
        Menu::with_items(app, &[&show_item, &quit_item]).map_err(|error| error.to_string())?;

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
                crate::utils::window::toggle_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|error| error.to_string())?;

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            crate::utils::window::show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = init_storage_dir(app)?;
            crate::utils::logging::init(&data_dir.join(LOG_FILE_NAME), DEFAULT_LOG_LEVEL)
                .map_err(|error| format!("Failed to initialize logging: {error}"))?;
            info!("Application setup started");
            debug!("App data directory: {}", data_dir.display());

            let settings_store = Arc::new(init_settings_store(&data_dir)?);
            let clipboard = init_clipboard_engine(app.handle(), &data_dir, &settings_store)?;
            let app_info = AppInfoState(services::app_info::build_app_info(app.handle()));
            let runtime_status = Arc::new(RuntimeStatusState(std::sync::Mutex::new(
                services::runtime::initial_status(),
            )));

            manage_app_state(
                app,
                ManagedAppState {
                    clipboard: clipboard.clone(),
                    settings_store,
                    runtime_status: runtime_status.clone(),
                    app_info,
                },
            );
            setup_tray_menu(app)?;

            let i18n = app.state::<Arc<RwLock<i18n::I18n>>>();
            if let Err(error) = services::settings::restore_settings_effects(
                app.handle(),
                &app.state::<Arc<SettingsStore>>(),
                app.state::<ClipboardEngineHandle>().inner(),
                &i18n,
            ) {
                warn!("Failed to restore settings effects: {error}");
            }
            if let Err(error) = services::persisted_state::restore_persisted_effects(
                app.handle(),
                &app.state::<Arc<SettingsStore>>(),
            ) {
                warn!("Failed to restore persisted effects: {error}");
            }

            services::runtime::initialize_system_theme(app.handle(), &runtime_status);
            ClipboardWatcher::new().start(WatcherStartContext {
                app_handle: app.handle().clone(),
                engine: clipboard,
                runtime_status,
            });

            if !std::env::args().any(|argument| argument == AUTOSTART_ARG) {
                crate::utils::window::show_main_window(app.handle());
            }
            apply_window_icon(app);
            info!("Application setup completed");
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::ThemeChanged(theme) if window.label() == MAIN_WINDOW_LABEL => {
                let app = window.app_handle();
                let runtime_status = app.state::<Arc<RuntimeStatusState>>();
                if let Err(error) =
                    services::runtime::report_system_theme(app, runtime_status.inner(), *theme)
                {
                    warn!("Failed to publish system theme change: {error}");
                }
            }
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if let Ok(position) = window.outer_position() {
                    let app = window.app_handle();
                    let store = app.state::<Arc<SettingsStore>>();
                    let i18n = app.state::<Arc<RwLock<i18n::I18n>>>();
                    if let Err(error) = services::persisted_state::save_persisted(
                        app,
                        &store,
                        &i18n,
                        PersistedStatePatch {
                            window_x: Some(Some(position.x)),
                            window_y: Some(Some(position.y)),
                            always_on_top: None,
                        },
                    ) {
                        warn!("Failed to persist window position: {error}");
                    }
                }
                api.prevent_close();
                crate::utils::window::hide_main_window(window);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_clipboard_list_items,
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
