// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// The global shortcut (default Ctrl+Alt+G).
//   Windows: RegisterHotKey.   X11: a key grab.
//   Wayland: the GlobalShortcuts portal. The desktop asks the user to confirm
//   it once and owns it from then on (GNOME: Settings › Apps › Grammable), so
//   changing it in Grammable's settings has no effect there.
// `grammable --popup` works everywhere and can be bound to a desktop shortcut.

use std::sync::Mutex;

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::app::AppState;
use crate::popup;

/// The shortcut currently registered, as written in settings.
static REGISTERED: Mutex<Option<String>> = Mutex::new(None);

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            // Act on press; the trigger waits for the modifiers to be released before copying.
            if event.state() == ShortcutState::Pressed {
                crate::platform::debug("hotkey", "pressed");
                popup::trigger(app, None);
            }
        })
        .build()
}

pub fn registered(_app: &AppHandle) -> Option<String> {
    REGISTERED.lock().unwrap().clone()
}

fn set_status(app: &AppHandle, error: Option<String>, note: Option<String>) {
    let state = app.state::<AppState>();
    *state.hotkey_error.lock().unwrap() = error;
    *state.hotkey_note.lock().unwrap() = note;
}

/// Replaces the current shortcut. Failures are kept for Settings to show.
pub fn register(app: &AppHandle, shortcut: &str) {
    let first = REGISTERED.lock().unwrap().replace(shortcut.to_string()).is_none();

    #[cfg(target_os = "linux")]
    if crate::platform::wayland::is_wayland() {
        // The desktop owns the binding after the first time; nothing to redo.
        if first {
            register_portal(app.clone(), shortcut.to_string());
        }
        return;
    }
    let _ = first;

    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    let result = manager.register(shortcut).map_err(|e| {
        format!("Could not register the shortcut {shortcut}: {e}. Another app may already use it.")
    });
    if let Err(e) = &result {
        eprintln!("Grammable: {e}");
    }
    crate::platform::debug("hotkey", format!("register {shortcut}: {:?}", result.as_ref().map(|_| "ok")));
    set_status(app, result.err(), None);
}

#[cfg(target_os = "linux")]
fn register_portal(app: AppHandle, shortcut: String) {
    tauri::async_runtime::spawn(async move {
        let handle = app.clone();
        let bound = crate::platform::wayland::bind_shortcut(&shortcut, move |token| {
            let app = handle.clone();
            // Portal signals arrive on the async runtime; the trigger blocks on keys and the clipboard.
            std::thread::spawn(move || popup::trigger(&app, token));
        })
        .await;
        match bound {
            Ok(trigger) => {
                let trigger = if trigger.is_empty() { shortcut.clone() } else { trigger };
                set_status(
                    &app,
                    None,
                    Some(format!(
                        "On Wayland your desktop manages this shortcut: {trigger}. \
                         Change it in GNOME Settings › Apps › Grammable."
                    )),
                );
            }
            Err(e) => {
                eprintln!("Grammable: {e}");
                set_status(
                    &app,
                    Some(format!("{e} You can bind a desktop shortcut to `grammable --popup` instead.")),
                    None,
                );
            }
        }
        crate::app::settings_changed(&app);
    });
}
