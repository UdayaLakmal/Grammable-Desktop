// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// The global shortcut (default Ctrl+Alt+G). Windows: RegisterHotKey.
// Linux: an X11 key grab, which on Wayland only fires while an X11 app has
// focus; the GlobalShortcuts portal comes in M3. `grammable --popup` works
// everywhere and can be bound to a desktop shortcut.

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
                popup::trigger(app);
            }
        })
        .build()
}

pub fn registered(_app: &AppHandle) -> Option<String> {
    REGISTERED.lock().unwrap().clone()
}

/// Replaces the current shortcut. Failures are kept for Settings to show.
pub fn register(app: &AppHandle, shortcut: &str) {
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    let result = manager.register(shortcut).map_err(|e| {
        format!("Could not register the shortcut {shortcut}: {e}. Another app may already use it.")
    });
    if let Err(e) = &result {
        eprintln!("Grammable: {e}");
    }
    *REGISTERED.lock().unwrap() = Some(shortcut.to_string());
    *app.state::<AppState>().hotkey_error.lock().unwrap() = result.err();
}
