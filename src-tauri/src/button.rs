// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// The floating "G" button shown next to a fresh selection (Windows and X11).
// It never takes focus, so the app you're writing in keeps its selection.
// Clicking it opens the action menu; it hides after 5 s unless the mouse is on it.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use crate::app::AppState;
use crate::guard::{self, Verdict};
use crate::platform::{self, Target};
use crate::popup;

pub const LABEL: &str = "trigger";
/// Window size in CSS pixels: a 28 px circle with room for its shadow.
const SIZE: f64 = 36.0;
const AUTO_HIDE: Duration = Duration::from_secs(5);

/// A selection the button would act on.
#[derive(Debug, Clone)]
pub struct Pending {
    /// `None` when the app doesn't expose its selection; it's copied on click.
    pub text: Option<String>,
    pub target: Option<Target>,
    /// Mouse position in physical pixels.
    pub anchor: (f64, f64),
}

#[derive(Default)]
pub struct Button {
    inner: Mutex<State>,
}

#[derive(Default)]
struct State {
    pending: Option<Pending>,
    /// Where the button is, in physical pixels: x, y, width, height.
    rect: Option<(f64, f64, f64, f64)>,
    hovering: bool,
    /// Bumped on every show, so an old auto-hide timer doesn't hide a newer button.
    generation: u64,
}

fn with<R>(app: &AppHandle, f: impl FnOnce(&mut State) -> R) -> R {
    let button = app.state::<Button>();
    let mut guard = button.inner.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

pub fn create_window(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("trigger.html".into()))
        .title("Grammable")
        .inner_size(SIZE, SIZE)
        .visible(false)
        .focused(false)
        .focusable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()?;
    Ok(())
}

/// Whether a physical screen point is on the button.
pub fn contains(app: &AppHandle, x: f64, y: f64) -> bool {
    with(app, |s| s.rect.is_some_and(|(rx, ry, w, h)| x >= rx && x < rx + w && y >= ry && y < ry + h))
}

pub fn show(app: &AppHandle, pending: Pending) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let (x, y) = pending.anchor;
    let scale = app.monitor_from_point(x, y).ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
    let size = SIZE * scale;
    // 6 px below-right of the mouse, like the macOS app.
    let (px, py) = ((x + 6.0 * scale).round(), (y + 6.0 * scale).round());
    let generation = with(app, |s| {
        s.pending = Some(pending);
        s.rect = Some((px, py, size, size));
        s.hovering = false;
        s.generation += 1;
        s.generation
    });
    let _ = window.set_position(PhysicalPosition::new(px, py));
    show_without_focus(&window);

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(AUTO_HIDE);
        // Still the same button and the mouse isn't on it.
        if with(&app, |s| s.generation == generation && !s.hovering) {
            hide(&app);
        }
    });
}

#[cfg(windows)]
fn show_without_focus(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
    match window.hwnd() {
        Ok(hwnd) => unsafe {
            ShowWindow(hwnd.0 as _, SW_SHOWNOACTIVATE);
        },
        Err(_) => {
            let _ = window.show();
        }
    }
}

#[cfg(not(windows))]
fn show_without_focus(window: &tauri::WebviewWindow) {
    let _ = window.show();
}

/// Hides with the same Win32 call that showed it. The button is shown behind
/// tao's back (to avoid taking focus), so tao still thinks it's hidden and
/// `window.hide()` is a no-op: it only acts when its own visible flag changes.
#[cfg(windows)]
fn hide_window(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    match window.hwnd() {
        Ok(hwnd) => unsafe {
            ShowWindow(hwnd.0 as _, SW_HIDE);
        },
        Err(_) => {
            let _ = window.hide();
        }
    }
}

#[cfg(not(windows))]
fn hide_window(window: &tauri::WebviewWindow) {
    let _ = window.hide();
}

pub fn hide(app: &AppHandle) {
    let was_visible = with(app, |s| {
        s.pending = None;
        s.hovering = false;
        s.rect.take().is_some()
    });
    if was_visible {
        if let Some(window) = app.get_webview_window(LABEL) {
            hide_window(&window);
        }
        platform::debug("button", "hidden");
    }
}

#[tauri::command]
pub fn button_hover(app: AppHandle, hovering: bool) {
    let generation = with(&app, |s| {
        s.hovering = hovering;
        s.generation
    });
    if !hovering {
        // Leaving the button restarts the countdown.
        std::thread::spawn(move || {
            std::thread::sleep(AUTO_HIDE);
            if with(&app, |s| s.generation == generation && !s.hovering) {
                hide(&app);
            }
        });
    }
}

#[derive(Serialize)]
pub struct Info {
    hotkey: String,
}

#[tauri::command]
pub fn button_info(state: tauri::State<AppState>) -> Info {
    Info { hotkey: state.settings().hotkey }
}

/// Opens the action menu for the selection the button was shown for.
#[tauri::command]
pub fn button_clicked(app: AppHandle) {
    let pending = with(&app, |s| s.pending.take());
    hide(&app);
    let Some(pending) = pending else { return };
    platform::debug("button", format!("clicked; text known: {}", pending.text.is_some()));
    std::thread::spawn(move || {
        let settings = app.state::<AppState>().settings();
        // The app didn't expose its selection: copy it now. It still has focus,
        // because the button never takes it.
        let text = pending.text.or_else(platform::copy_selection);
        match guard::check_text(text.as_deref(), &settings) {
            Verdict::Ok => popup::open_menu(&app, text.unwrap_or_default(), pending.target, Some(pending.anchor)),
            _ => {} // nothing usable was copied; stay quiet
        }
    });
}
