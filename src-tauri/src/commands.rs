// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Commands the web UI calls through `window.__TAURI__.core.invoke`.
// Errors are returned as plain strings for display.

use grammable_core::backends::{self, http};
use grammable_core::{BackendId, EngineResult, Preset};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::app::{self, AppState};
use crate::settings::Settings;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> CmdResult<()> {
    settings.save().map_err(|e| format!("Could not save settings: {e}"))?;
    *state.settings.lock().unwrap() = settings;
    app::settings_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn get_launch_at_login(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_launch_at_login(app: AppHandle, enabled: bool) -> CmdResult<bool> {
    let launcher = app.autolaunch();
    if enabled { launcher.enable() } else { launcher.disable() }.map_err(err)?;
    launcher.is_enabled().map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    id: BackendId,
    name: &'static str,
    short_name: &'static str,
    on_device: bool,
    can_rewrite: bool,
    privacy_note: &'static str,
    setup_hint: &'static str,
    setup_url: &'static str,
    is_cli: bool,
    available: bool,
    detail: String,
    /// Installed / loaded models, for Ollama and OpenAI-compatible servers.
    models: Vec<String>,
}

/// Status of every backend, checked concurrently. Never prompts the user.
#[tauri::command]
pub async fn backend_statuses(state: State<'_, AppState>) -> CmdResult<Vec<BackendInfo>> {
    let config = state.settings().backends;
    let tasks: Vec<_> = BackendId::ALL
        .into_iter()
        .map(|id| {
            let config = config.clone();
            tauri::async_runtime::spawn(async move {
                let status = backends::check_availability(id, &config).await;
                let models = match id {
                    BackendId::Ollama if status.available => http::ollama_models(&config).await.unwrap_or_default(),
                    BackendId::OpenAiCompatible if status.available => {
                        http::openai_models(&config).await.unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                BackendInfo {
                    id,
                    name: id.display_name(),
                    short_name: id.short_name(),
                    on_device: id.is_on_device(),
                    can_rewrite: id.can_rewrite(),
                    privacy_note: id.privacy_note(),
                    setup_hint: id.setup_hint(),
                    setup_url: id.setup_url(),
                    is_cli: id.executable_name().is_some(),
                    available: status.available,
                    detail: status.detail,
                    models,
                }
            })
        })
        .collect();
    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        out.push(task.await.map_err(err)?);
    }
    Ok(out)
}

pub const TEST_SENTENCE: &str = "this sentense have two mistake.";

/// Runs the grammar preset on a sample sentence with one backend (Settings › Test).
#[tauri::command]
pub async fn test_backend(state: State<'_, AppState>, id: BackendId) -> CmdResult<EngineResult> {
    let config = state.settings().backends;
    let mut preset = grammable_core::presets::built_in().into_iter().find(|p| p.id == "fix-grammar").ok_or("missing preset")?;
    preset.backend = Some(id.as_str().into());
    // No fallback: the point is to test this backend.
    grammable_core::run_preset(&preset, TEST_SENTENCE, id, &config, false).await.map_err(err)
}

#[tauri::command]
pub fn default_excluded_apps() -> Vec<String> {
    crate::settings::default_excluded_apps()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetList {
    presets: Vec<PresetInfo>,
    errors: Vec<String>,
    directory: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    #[serde(flatten)]
    preset: Preset,
    /// The backend this preset will actually use with the current default.
    resolved_backend: BackendId,
    /// A user file replaces a built-in preset with the same id.
    customized: bool,
}

#[tauri::command]
pub fn load_presets(state: State<AppState>) -> PresetList {
    let default = state.settings().default_backend;
    let store = app::preset_store();
    let result = store.load();
    let built_in: Vec<String> = grammable_core::presets::built_in().into_iter().map(|p| p.id).collect();
    PresetList {
        presets: result
            .presets
            .into_iter()
            .map(|p| PresetInfo {
                resolved_backend: grammable_core::engine::resolve_backend(&p, default),
                customized: !p.is_built_in && built_in.contains(&p.id),
                preset: p,
            })
            .collect(),
        errors: result.errors,
        directory: store.directory.display().to_string(),
    }
}

pub fn reveal_presets_folder(app: &AppHandle) -> CmdResult<()> {
    let dir = app::preset_store().ensure_directory().map_err(err)?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
pub fn open_presets_folder(app: AppHandle) -> CmdResult<()> {
    reveal_presets_folder(&app)
}

fn open_file(app: &AppHandle, path: &std::path::Path) -> CmdResult<()> {
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(err)
}

/// Copies a built-in preset to the presets folder and opens it in the system editor.
#[tauri::command]
pub fn customize_preset(app: AppHandle, id: String) -> CmdResult<()> {
    let path = app::preset_store()
        .export_built_in(&id)
        .map_err(err)?
        .ok_or_else(|| format!("“{id}” is not a built-in preset."))?;
    open_file(&app, &path)
}

/// Opens a user preset file in the system editor.
#[tauri::command]
pub fn edit_preset(app: AppHandle, id: String) -> CmdResult<()> {
    let preset = app::preset_store().load().presets.into_iter().find(|p| p.id == id);
    match preset.and_then(|p| p.file) {
        Some(path) => open_file(&app, &path),
        None => customize_preset(app, id),
    }
}

#[tauri::command]
pub fn create_preset(app: AppHandle, name: String) -> CmdResult<()> {
    let path = app::preset_store().create_preset(name.trim()).map_err(err)?;
    open_file(&app, &path)
}

/// Opens an http(s) link in the default browser.
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    /// Why the shortcut couldn't be registered, if it couldn't.
    error: Option<String>,
    /// E.g. on Wayland: the shortcut the desktop actually uses, and where to change it.
    note: Option<String>,
}

#[tauri::command]
pub fn hotkey_status(state: State<AppState>) -> HotkeyStatus {
    HotkeyStatus {
        error: state.hotkey_error.lock().unwrap().clone(),
        note: state.hotkey_note.lock().unwrap().clone(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    platform: &'static str,
    /// "windows", "x11", "wayland" or "unknown".
    session: String,
    settings_path: String,
    releases_url: &'static str,
    kofi_url: &'static str,
    repository: &'static str,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        session: session_type(),
        settings_path: Settings::path().display().to_string(),
        releases_url: app::RELEASES_URL,
        kofi_url: app::KOFI_URL,
        repository: env!("CARGO_PKG_REPOSITORY"),
    }
}

pub fn session_type() -> String {
    if cfg!(windows) {
        return "windows".into();
    }
    match std::env::var("XDG_SESSION_TYPE").map(|s| s.to_lowercase()) {
        Ok(s) if s == "wayland" || s == "x11" => s,
        _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => "wayland".into(),
        _ if std::env::var_os("DISPLAY").is_some() => "x11".into(),
        _ => "unknown".into(),
    }
}

// MARK: Wayland

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaylandStatus {
    /// The user has allowed keyboard access before (a restore token is saved).
    allowed: bool,
    /// A portal session is running.
    connected: bool,
}

#[tauri::command]
pub async fn wayland_status() -> WaylandStatus {
    #[cfg(target_os = "linux")]
    {
        use crate::platform::wayland;
        WaylandStatus { allowed: wayland::has_saved_consent(), connected: wayland::is_connected().await }
    }
    #[cfg(not(target_os = "linux"))]
    WaylandStatus { allowed: false, connected: false }
}

/// Starts the portal session, which shows the desktop's consent dialog if needed.
#[tauri::command]
pub async fn wayland_connect() -> CmdResult<()> {
    #[cfg(target_os = "linux")]
    return crate::platform::wayland::connect().await;
    #[cfg(not(target_os = "linux"))]
    Ok(())
}

/// Ends the session and forgets the consent; the desktop asks again next time.
#[tauri::command]
pub async fn wayland_reset() {
    #[cfg(target_os = "linux")]
    crate::platform::wayland::disconnect().await;
}
