// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// The desktop app: tray icon and menu, Settings window, single instance,
// launch at login, first-run setup. Lives in the tray; closing windows never quits.

use std::sync::Mutex;

use grammable_core::{backends, BackendId, PresetStore};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_opener::OpenerExt;

use crate::settings::Settings;
use crate::{commands, hotkey, popup};

pub const RELEASES_URL: &str = "https://github.com/UdayaLakmal/Grammable-Desktop/releases";
pub const KOFI_URL: &str = "https://ko-fi.com/udayalakmal";

/// Started by the login item: stay in the tray, don't open Settings.
pub const ARG_BACKGROUND: &str = "--background";
/// Ask the running instance to act on the current selection (e.g. from a desktop shortcut).
pub const ARG_POPUP: &str = "--popup";

const TRAY_ID: &str = "main";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");
const TRAY_ICON_PAUSED: &[u8] = include_bytes!("../icons/tray-paused.png");

pub struct AppState {
    pub settings: Mutex<Settings>,
    /// Why the global shortcut couldn't be registered, if it couldn't.
    pub hotkey_error: Mutex<Option<String>>,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
}

pub fn run(args: &[String]) {
    let background = args.iter().any(|a| a == ARG_BACKGROUND);
    let (settings, warning) = Settings::load();
    if let Some(w) = warning {
        eprintln!("Warning: {w}");
    }
    let first_run = !Settings::exists();
    let popup = args.iter().any(|a| a == ARG_POPUP);
    // Plain launch → Settings. Login item and `--popup` stay quiet, except until onboarding is done.
    let show_settings_now = !background && (!popup || !settings.has_completed_onboarding);

    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch forwards its arguments here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if argv.iter().any(|a| a == ARG_POPUP) {
                trigger_from_hotkey(app);
            } else {
                show_settings(app, None);
            }
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![ARG_BACKGROUND])))
        .plugin(tauri_plugin_opener::init())
        .plugin(hotkey::plugin())
        .manage(AppState { settings: Mutex::new(settings), hotkey_error: Mutex::new(None) })
        .manage(popup::Popup::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_launch_at_login,
            commands::set_launch_at_login,
            commands::backend_statuses,
            commands::test_backend,
            commands::default_excluded_apps,
            commands::load_presets,
            commands::open_presets_folder,
            commands::customize_preset,
            commands::edit_preset,
            commands::create_preset,
            commands::open_url,
            commands::app_info,
            commands::hotkey_status,
            popup::popup_context,
            popup::popup_resize,
            popup::popup_run,
            popup::popup_cancel,
            popup::popup_dismiss,
            popup::popup_replace,
            popup::popup_copy,
            popup::popup_open_settings,
        ])
        .setup(move |app| {
            build_tray(app.handle())?;
            popup::create_window(app.handle())?;
            let shortcut = app.state::<AppState>().settings().hotkey;
            hotkey::register(app.handle(), &shortcut);
            if first_run {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { first_run_setup(handle).await });
            }
            if show_settings_now {
                show_settings(app.handle(), None);
            } else if popup {
                trigger_from_hotkey(app.handle());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to start Grammable");

    app.run(|_app, event| {
        // Closing the last window must not quit: Grammable lives in the tray.
        // `app.exit()` (the Quit item) passes an exit code, which is let through.
        if let RunEvent::ExitRequested { code: None, api, .. } = event {
            api.prevent_exit();
        }
    });
}

/// First launch: pick the first AI that is ready as the default, then save.
async fn first_run_setup(app: AppHandle) {
    let state = app.state::<AppState>();
    let config = state.settings().backends;
    let backend = backends::first_available(&config).await;
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.default_backend = backend;
        s.clone()
    };
    if let Err(e) = snapshot.save() {
        eprintln!("Warning: could not save settings: {e}");
    }
    settings_changed(&app);
}

/// Call after any settings change: refresh the tray and tell open windows.
pub fn settings_changed(app: &AppHandle) {
    let shortcut = app.state::<AppState>().settings().hotkey;
    if hotkey::registered(app).as_deref() != Some(shortcut.as_str()) {
        hotkey::register(app, &shortcut);
    }
    if let Err(e) = refresh_tray(app) {
        eprintln!("Warning: could not update the tray menu: {e}");
    }
    let _ = app.emit("settings-changed", app.state::<AppState>().settings());
}

// MARK: Tray

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = app.state::<AppState>().settings();
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon(settings.is_enabled)?)
        .tooltip("Grammable")
        .menu(&tray_menu(app, &settings)?)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;
    Ok(())
}

fn tray_icon(enabled: bool) -> tauri::Result<Image<'static>> {
    Image::from_bytes(if enabled { TRAY_ICON } else { TRAY_ICON_PAUSED })
}

fn refresh_tray(app: &AppHandle) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return Ok(()) };
    let settings = app.state::<AppState>().settings();
    tray.set_menu(Some(tray_menu(app, &settings)?))?;
    tray.set_icon(Some(tray_icon(settings.is_enabled)?))?;
    tray.set_tooltip(Some(if settings.is_enabled { "Grammable" } else { "Grammable (paused)" }))?;
    Ok(())
}

fn tray_menu(app: &AppHandle, s: &Settings) -> tauri::Result<Menu<Wry>> {
    let backend_items = BackendId::ALL
        .iter()
        .map(|id| {
            let place = if id.is_on_device() { "🔒" } else { "☁" };
            CheckMenuItem::with_id(
                app,
                format!("backend:{}", id.as_str()),
                format!("{place}  {}", id.display_name()),
                true,
                *id == s.default_backend,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let mut backend_refs: Vec<&dyn IsMenuItem<Wry>> = backend_items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    let sep_ai = PredefinedMenuItem::separator(app)?;
    let ai_settings = MenuItem::with_id(app, "ai-settings", "AI Backend Settings…", true, None::<&str>)?;
    backend_refs.push(&sep_ai);
    backend_refs.push(&ai_settings);
    let default_ai = Submenu::with_items(app, format!("Default AI: {}", s.default_backend.short_name()), true, &backend_refs)?;

    let title = MenuItem::with_id(app, "title", "Grammable", false, None::<&str>)?;
    let enabled = CheckMenuItem::with_id(app, "enabled", "Enabled", true, s.is_enabled, None::<&str>)?;
    let button = CheckMenuItem::with_id(
        app,
        "popup-on-selection",
        "Show Button on Text Selection",
        s.is_enabled,
        s.popup_on_selection,
        None::<&str>,
    )?;
    let improve = MenuItem::with_id(app, "improve", format!("Improve Selected Text ({})", s.hotkey), s.is_enabled, None::<&str>)?;
    let edit_presets = MenuItem::with_id(app, "edit-presets", "Edit Presets…", true, None::<&str>)?;
    let presets_folder = MenuItem::with_id(app, "presets-folder", "Open Presets Folder", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let updates = MenuItem::with_id(app, "updates", "Check for Updates…", true, None::<&str>)?;
    let kofi = MenuItem::with_id(app, "kofi", "☕ Support Grammable on Ko-fi", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Grammable", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &title,
            &PredefinedMenuItem::separator(app)?,
            &enabled,
            &button,
            &improve,
            &PredefinedMenuItem::separator(app)?,
            &default_ai,
            &edit_presets,
            &presets_folder,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &updates,
            &kofi,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    let state = app.state::<AppState>();
    let update = |change: &dyn Fn(&mut Settings)| {
        let snapshot = {
            let mut s = state.settings.lock().unwrap();
            change(&mut s);
            s.clone()
        };
        if let Err(e) = snapshot.save() {
            eprintln!("Warning: could not save settings: {e}");
        }
        settings_changed(app);
    };

    match id {
        "enabled" => update(&|s| s.is_enabled = !s.is_enabled),
        "popup-on-selection" => update(&|s| s.popup_on_selection = !s.popup_on_selection),
        "improve" => trigger_from_hotkey(app),
        "ai-settings" => show_settings(app, Some("backends")),
        "edit-presets" => show_settings(app, Some("presets")),
        "presets-folder" => {
            if let Err(e) = commands::reveal_presets_folder(app) {
                eprintln!("Warning: {e}");
            }
        }
        "settings" => show_settings(app, None),
        "updates" => open_url(app, RELEASES_URL),
        "kofi" => open_url(app, KOFI_URL),
        "quit" => app.exit(0),
        other => {
            if let Some(backend) = other.strip_prefix("backend:").and_then(BackendId::parse) {
                update(&|s| s.default_backend = backend);
            }
        }
    }
}

pub fn open_url(app: &AppHandle, url: &str) {
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        eprintln!("Warning: could not open {url}: {e}");
    }
}

/// Hotkey / tray / `--popup`: act on the current selection.
pub fn trigger_from_hotkey(app: &AppHandle) {
    popup::trigger(app);
}

// MARK: Settings window

pub fn show_settings(app: &AppHandle, tab: Option<&str>) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(tab) = tab {
            let _ = window.emit("open-tab", tab);
        }
        return;
    }
    let url = match tab {
        Some(tab) => format!("settings.html#{tab}"),
        None => "settings.html".into(),
    };
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App(url.into()))
        .title("Grammable Settings")
        .inner_size(680.0, 640.0)
        .min_inner_size(560.0, 480.0)
        .center()
        .build();
    match built {
        Ok(window) => {
            // Closing only hides it, so it reopens instantly and keeps its tab.
            let w = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w.hide();
                }
            });
        }
        Err(e) => eprintln!("Warning: could not open Settings: {e}"),
    }
}

/// Presets folder, created on first use.
pub fn preset_store() -> PresetStore {
    PresetStore::default()
}
