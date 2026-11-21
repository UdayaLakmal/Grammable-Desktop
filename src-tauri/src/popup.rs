// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// The popup: action menu → working → review, or a message. The window is
// created hidden at startup and reused. Rust owns the selection and the
// target window; the page (ui/popup.js) owns what is on screen and asks Rust
// to size it, run presets, replace and copy.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use grammable_core::{BackendId, EngineResult, Preset};
use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder};

use crate::app::{self, AppState};
use crate::guard::{self, Verdict};
use crate::platform::{self, Probe, Target};

pub const LABEL: &str = "popup";

/// What the page should show.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum View {
    Menu,
    #[serde(rename_all = "camelCase")]
    Message { title: String, detail: String, is_error: bool },
}

#[derive(Default)]
pub struct Popup {
    inner: Mutex<Inner>,
    /// A capture is in progress (the hotkey can repeat while held).
    capturing: AtomicBool,
}

#[derive(Default)]
struct Inner {
    view: Option<View>,
    text: String,
    target: Option<Target>,
    /// Mouse position when invoked, in physical pixels; `None` where unknown (Wayland).
    anchor: Option<(f64, f64)>,
    /// Dropping or sending on this cancels the running request.
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    /// The window has had focus since it was shown, so losing it means "clicked elsewhere".
    had_focus: bool,
    /// Wayland: the token the desktop passed with the shortcut, which lets the popup take focus.
    activation_token: Option<String>,
}

impl Popup {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// MARK: Window

pub fn create_window(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("popup.html".into()))
        .title("Grammable")
        .inner_size(280.0, 320.0)
        .visible(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(true)
        .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(true) => handle.state::<Popup>().lock().had_focus = true,
        // Clicking anywhere else closes the popup, so Replace can never paste into the wrong place.
        tauri::WindowEvent::Focused(false) => {
            if handle.state::<Popup>().lock().had_focus {
                dismiss(&handle);
            }
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            dismiss(&handle);
        }
        _ => {}
    });
    Ok(())
}

fn open(app: &AppHandle, view: View) {
    {
        let popup = app.state::<Popup>();
        let mut inner = popup.lock();
        inner.view = Some(view.clone());
        inner.had_focus = false;
    }
    // The page renders, measures itself and calls `popup_resize`, which shows the window.
    let _ = app.emit_to(LABEL, "popup-open", view);
}

/// Opens the action menu for text captured elsewhere (the G button).
pub fn open_menu(app: &AppHandle, text: String, target: Option<Target>, anchor: Option<(f64, f64)>) {
    {
        let popup = app.state::<Popup>();
        let mut inner = popup.lock();
        inner.text = text;
        inner.target = target;
        inner.anchor = anchor;
    }
    open(app, View::Menu);
}

pub fn dismiss(app: &AppHandle) {
    {
        let popup = app.state::<Popup>();
        let mut inner = popup.lock();
        if inner.view.is_none() {
            return;
        }
        inner.view = None;
        inner.cancel = None;
        inner.text.clear();
        inner.target = None;
        inner.had_focus = false;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    let _ = app.emit_to(LABEL, "popup-closed", ());
}

pub fn is_visible(app: &AppHandle) -> bool {
    app.state::<Popup>().lock().view.is_some()
}

/// Where the popup goes: 10 px below the mouse, above it if there's no room,
/// kept 8 px inside the work area. All in physical pixels.
pub fn place(anchor: (f64, f64), size: (f64, f64), area: (f64, f64, f64, f64), gap: f64) -> (f64, f64) {
    let (ax, ay) = anchor;
    let (w, h) = size;
    let (left, top, width, height) = area;
    let (right, bottom) = (left + width, top + height);
    let margin = 8.0 * (gap / 10.0).max(0.1);
    let mut x = ax - 12.0 * gap / 10.0;
    let mut y = ay + gap;
    if y + h > bottom - margin {
        y = ay - gap - h; // no room below → above
    }
    x = x.min(right - w - margin).max(left + margin);
    y = y.min(bottom - h - margin).max(top + margin);
    (x.round(), y.round())
}

// MARK: Trigger

/// Hotkey, tray item or `grammable --popup`: read the selection and show the menu.
/// `activation_token` comes from the Wayland shortcut portal.
pub fn trigger(app: &AppHandle, activation_token: Option<String>) {
    let state = app.state::<AppState>().settings();
    if !state.is_enabled {
        return;
    }
    if is_visible(app) {
        dismiss(app); // pressing the shortcut again closes it
        return;
    }
    crate::button::hide(app);
    let popup = app.state::<Popup>();
    if popup.capturing.swap(true, Ordering::SeqCst) {
        return;
    }
    popup.lock().activation_token = activation_token;
    // Wayland doesn't reveal the mouse position; the desktop places the popup.
    let anchor = if is_wayland() {
        None
    } else {
        app.cursor_position().ok().map(|p| (p.x, p.y)).filter(|p| *p != (0.0, 0.0))
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let view = capture(&app, anchor);
        app.state::<Popup>().capturing.store(false, Ordering::SeqCst);
        if let Some(view) = view {
            open(&app, view);
        }
    });
}

fn is_wayland() -> bool {
    #[cfg(target_os = "linux")]
    return platform::wayland::is_wayland();
    #[cfg(not(target_os = "linux"))]
    false
}

/// Wayland: copy through the portal. No excluded-app check is possible, so it
/// copies with Ctrl+Insert, which doesn't interrupt programs in terminals.
#[cfg(target_os = "linux")]
fn capture_wayland(app: &AppHandle, settings: &crate::settings::Settings) -> Option<View> {
    // Give the user a moment to let go of the shortcut's keys.
    std::thread::sleep(Duration::from_millis(250));
    let text = match tauri::async_runtime::block_on(platform::wayland::copy_selection()) {
        Ok(text) => text,
        Err(e) => {
            return Some(View::Message {
                title: "Grammable needs keyboard access".into(),
                detail: format!(
                    "{e}\n\nOn Wayland, Grammable copies and pastes through your desktop's remote-input \
                     permission. Allow it in Grammable Settings › General."
                ),
                is_error: true,
            });
        }
    };
    if let Verdict::Refuse { title, detail } = guard::check_text(text.as_deref(), settings) {
        return Some(View::Message { title, detail, is_error: false });
    }
    let popup = app.state::<Popup>();
    let mut inner = popup.lock();
    inner.text = text.unwrap_or_default();
    inner.target = None;
    inner.anchor = None;
    Some(View::Menu)
}

/// Runs off the main thread: it waits for keys and the clipboard.
fn capture(app: &AppHandle, anchor: Option<(f64, f64)>) -> Option<View> {
    let settings = app.state::<AppState>().settings();
    #[cfg(target_os = "linux")]
    if is_wayland() {
        return capture_wayland(app, &settings);
    }
    // Let go of Ctrl+Alt first, or the Ctrl+C below would arrive as Ctrl+Alt+C.
    platform::wait_for_modifiers_released(Duration::from_millis(1500));

    let target = platform::foreground();
    let message = |title: String, detail: String| {
        let popup = app.state::<Popup>();
        let mut inner = popup.lock();
        inner.anchor = anchor;
        inner.target = None;
        Some(View::Message { title, detail, is_error: false })
    };
    match guard::check_target(target.as_ref(), &settings) {
        Verdict::Ok => {}
        Verdict::Ignore => return None,
        Verdict::Refuse { title, detail } => return message(title, detail),
    }

    let text = match platform::probe_selection() {
        Probe::Password => return None, // never read password fields
        Probe::Text(text) => Some(text),
        Probe::Unknown => platform::copy_selection(),
    };
    match guard::check_text(text.as_deref(), &settings) {
        Verdict::Ok => {}
        Verdict::Ignore => return None,
        Verdict::Refuse { title, detail } => return message(title, detail),
    }

    let popup = app.state::<Popup>();
    let mut inner = popup.lock();
    inner.text = text.unwrap_or_default();
    inner.target = target;
    inner.anchor = anchor;
    Some(View::Menu)
}

// MARK: Commands

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetItem {
    id: String,
    name: String,
    icon: String,
    mode: grammable_core::PresetMode,
    backend: BackendId,
    backend_name: &'static str,
    on_device: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopupContext {
    view: Option<View>,
    word_count: usize,
    presets: Vec<PresetItem>,
    default_backend: BackendId,
    default_backend_name: &'static str,
    default_on_device: bool,
    hotkey: String,
}

#[tauri::command]
pub fn popup_context(state: State<AppState>, popup: State<Popup>) -> PopupContext {
    let settings = state.settings();
    let (view, words) = {
        let inner = popup.lock();
        (inner.view.clone(), inner.text.split_whitespace().count())
    };
    let default = settings.default_backend;
    let presets = app::preset_store()
        .load()
        .presets
        .into_iter()
        .map(|p| {
            let backend = grammable_core::engine::resolve_backend(&p, default);
            PresetItem {
                id: p.id,
                name: p.name,
                icon: p.icon,
                mode: p.mode,
                backend,
                backend_name: backend.short_name(),
                on_device: backend.is_on_device(),
            }
        })
        .collect();
    PopupContext {
        view,
        word_count: words,
        presets,
        default_backend: default,
        default_backend_name: default.display_name(),
        default_on_device: default.is_on_device(),
        hotkey: settings.hotkey,
    }
}

/// Sizes the popup to the page's content (CSS pixels), places it and shows it.
#[tauri::command]
pub fn popup_resize(app: AppHandle, popup: State<Popup>, width: f64, height: f64) -> Result<(), String> {
    let anchor = {
        let inner = popup.lock();
        if inner.view.is_none() {
            return Ok(());
        }
        inner.anchor
    };
    let window = app.get_webview_window(LABEL).ok_or("no popup window")?;
    window.set_size(LogicalSize::new(width, height)).map_err(|e| e.to_string())?;
    match anchor.and_then(|(x, y)| app.monitor_from_point(x, y).ok().flatten().map(|m| (x, y, m))) {
        Some((x, y, monitor)) => {
            let scale = monitor.scale_factor();
            let area = monitor.work_area();
            let (px, py) = place(
                (x, y),
                (width * scale, height * scale),
                (area.position.x as f64, area.position.y as f64, area.size.width as f64, area.size.height as f64),
                10.0 * scale,
            );
            let _ = window.set_position(PhysicalPosition::new(px, py));
        }
        None => {
            let _ = window.center();
        }
    }
    if !window.is_visible().unwrap_or(false) {
        #[cfg(target_os = "linux")]
        if let Some(token) = popup.lock().activation_token.take() {
            // Wayland: without the shortcut's activation token, GNOME shows
            // "Grammable is ready" instead of focusing the popup.
            if let Ok(gtk_window) = window.gtk_window() {
                use gtk::prelude::GtkWindowExt;
                gtk_window.set_startup_id(&token);
            }
        }
        window.show().map_err(|e| e.to_string())?;
    }
    let _ = window.set_focus();
    Ok(())
}

/// Runs a preset on the captured text. Returns `Err("cancelled")` if cancelled.
#[tauri::command]
pub async fn popup_run(app: AppHandle, id: String) -> Result<EngineResult, String> {
    let settings = app.state::<AppState>().settings();
    let preset: Preset = app::preset_store()
        .load()
        .presets
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("The preset “{id}” no longer exists."))?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let text = {
        let popup = app.state::<Popup>();
        let mut inner = popup.lock();
        inner.cancel = Some(tx); // replaces (and so cancels) any earlier run
        inner.text.clone()
    };
    if text.is_empty() {
        return Err("cancelled".into());
    }
    let run = grammable_core::run_preset(
        &preset,
        &text,
        settings.default_backend,
        &settings.backends,
        settings.fallback_to_built_in,
    );
    // Dropping `run` on cancel also stops the CLI process it started.
    tokio::select! {
        result = run => result.map_err(|e| e.to_string()),
        _ = rx => Err("cancelled".into()),
    }
}

#[tauri::command]
pub fn popup_cancel(popup: State<Popup>) {
    popup.lock().cancel = None;
}

#[tauri::command]
pub fn popup_dismiss(app: AppHandle) {
    dismiss(&app);
}

/// Pastes the final text over the selection in the original window.
#[tauri::command]
pub fn popup_replace(app: AppHandle, text: String) {
    let target = app.state::<Popup>().lock().target.clone();
    dismiss(&app);
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        if is_wayland() {
            // Let focus go back to the app the text came from.
            std::thread::sleep(Duration::from_millis(250));
            if let Err(e) = tauri::async_runtime::block_on(platform::wayland::paste(&text)) {
                eprintln!("Grammable: {e}");
                let copied = tauri::async_runtime::block_on(platform::wayland::copy_text(&text)).is_ok();
                open(
                    &app,
                    View::Message {
                        title: "Couldn’t paste".into(),
                        detail: if copied {
                            format!("{e}\n\nThe text is on the clipboard: press Ctrl+V to paste it.")
                        } else {
                            e
                        },
                        is_error: true,
                    },
                );
            }
            return;
        }
        if let Err(e) = platform::paste(&text, target.as_ref()) {
            eprintln!("Grammable: {e}");
        }
    });
}

#[tauri::command]
pub fn popup_copy(app: AppHandle, text: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let ok = if is_wayland() {
        tauri::async_runtime::block_on(platform::wayland::copy_text(&text)).is_ok()
    } else {
        platform::clipboard::set_text(&text, false)
    };
    #[cfg(not(target_os = "linux"))]
    let ok = platform::clipboard::set_text(&text, false);
    dismiss(&app);
    if ok { Ok(()) } else { Err("Could not copy to the clipboard.".into()) }
}

#[tauri::command]
pub fn popup_open_settings(app: AppHandle, tab: Option<String>) {
    dismiss(&app);
    app::show_settings(&app, tab.as_deref());
}

#[cfg(test)]
mod tests {
    use super::place;

    const AREA: (f64, f64, f64, f64) = (0.0, 0.0, 1920.0, 1040.0);

    #[test]
    fn below_the_mouse() {
        assert_eq!(place((500.0, 300.0), (270.0, 200.0), AREA, 10.0), (488.0, 310.0));
    }

    #[test]
    fn above_when_no_room_below() {
        assert_eq!(place((500.0, 1000.0), (270.0, 200.0), AREA, 10.0), (488.0, 790.0));
    }

    #[test]
    fn kept_inside_the_work_area() {
        assert_eq!(place((1910.0, 5.0), (270.0, 200.0), AREA, 10.0), (1642.0, 15.0));
        assert_eq!(place((2.0, 300.0), (270.0, 200.0), AREA, 10.0), (8.0, 310.0));
        // Second monitor to the right, scaled 2x.
        let area = (1920.0, 0.0, 2560.0, 1400.0);
        assert_eq!(place((1925.0, 100.0), (540.0, 400.0), area, 20.0), (1936.0, 120.0));
    }
}
