// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// What Grammable needs from the operating system: which window is in front,
// what is selected there, and a way to paste back into it.
//
//   Windows: Win32 + UI Automation, keystrokes through SendInput.
//   Linux:   X11 (EWMH properties, XTest keystrokes). On Wayland these only
//            reach X11 apps running under XWayland; the portal path is M3.

pub mod clipboard;
mod keys;

#[cfg(target_os = "linux")]
pub mod wayland;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
use x11 as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

use std::thread::sleep;

/// With `GRAMMABLE_DEBUG=1`, logs what Grammable is doing (never the text itself).
pub fn debug(area: &str, message: impl std::fmt::Display) {
    if std::env::var_os("GRAMMABLE_DEBUG").is_some() {
        eprintln!("[{area}] {message}");
    }
}
use std::time::{Duration, Instant};

/// The window the user was writing in when Grammable was invoked.
#[derive(Debug, Clone)]
pub struct Target {
    /// HWND on Windows, X11 window id on Linux.
    pub(crate) handle: u64,
    pub pid: Option<u32>,
    /// Executable name without extension, e.g. `notepad`, `gnome-terminal-server`.
    pub process: Option<String>,
}

impl Target {
    pub fn is_self(&self) -> bool {
        self.pid == Some(std::process::id())
    }
}

/// What can be learned about the selection without touching the clipboard.
#[derive(Debug)]
#[cfg_attr(not(windows), allow(dead_code))] // X11 always copies
pub enum Probe {
    /// The focused field is a password field: never read it.
    Password,
    Text(String),
    /// The app doesn't expose its selection; copying is the only way.
    Unknown,
}

pub fn foreground() -> Option<Target> {
    imp::foreground()
}

/// Brings `target` back to the front. Returns false if the system refused.
pub fn activate(target: &Target) -> bool {
    imp::activate(target)
}

pub fn probe_selection() -> Probe {
    imp::probe_selection()
}

/// X11: the PRIMARY selection, which holds whatever was selected last.
/// Only meaningful right after the user selected something.
pub fn read_primary() -> Option<String> {
    #[cfg(target_os = "linux")]
    return clipboard::get_primary();
    #[cfg(not(target_os = "linux"))]
    None
}

/// Waits until Ctrl, Alt, Shift and Super are up, so a shortcut's own keys
/// don't mix with the Ctrl+C / Ctrl+V Grammable sends. Gives up after `timeout`.
pub fn wait_for_modifiers_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while imp::modifiers_held() {
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(15));
    }
    true
}

/// Copies the selection with Ctrl+C and reads it, restoring the clipboard afterwards.
/// Returns `None` if nothing was copied within 500 ms (usually: nothing selected).
pub fn copy_selection() -> Option<String> {
    let saved = clipboard::save();
    // A marker we can recognise, so an unchanged clipboard isn't mistaken for a copy.
    let marker = format!("\u{2063}grammable-{:?}", Instant::now());
    if !clipboard::set_text(&marker, true) {
        return None;
    }
    let copied = keys::copy().ok().and_then(|_| {
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            sleep(Duration::from_millis(25));
            match clipboard::get_text() {
                Some(text) if text != marker => return Some(text),
                _ if Instant::now() >= deadline => return None,
                _ => {}
            }
        }
    });
    clipboard::restore(saved);
    copied
}

/// Pastes `text` into `target` through the clipboard, then puts the old clipboard back.
pub fn paste(text: &str, target: Option<&Target>) -> Result<(), String> {
    if let Some(target) = target {
        if !activate(target) {
            eprintln!("Grammable: could not bring the original window back to the front");
        }
        sleep(Duration::from_millis(120));
    }
    let saved = clipboard::save();
    if !clipboard::set_text(text, true) {
        return Err("Could not put the text on the clipboard.".into());
    }
    keys::paste()?;
    // Give the app time to read the clipboard before restoring it.
    sleep(Duration::from_millis(450));
    // Only restore if nobody else has changed the clipboard in the meantime.
    if clipboard::get_text().as_deref() == Some(text) {
        clipboard::restore(saved);
    }
    Ok(())
}
