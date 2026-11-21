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

/// With `GRAMMABLE_DEBUG=1`, logs what Grammable is doing — app names, lengths
/// and outcomes, never the text itself — to stderr and to `debug.log` next to
/// settings.json (Windows has no console for a tray app).
pub fn debug(area: &str, message: impl std::fmt::Display) {
    if std::env::var_os("GRAMMABLE_DEBUG").is_none() {
        return;
    }
    let line = format!("[{area}] {message}");
    eprintln!("{line}");
    use std::io::Write;
    static FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);
    let mut file = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if file.is_none() {
        let path = debug_log_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Start fresh each run, so the file only holds this session.
        *file = std::fs::File::create(&path).ok();
    }
    if let Some(f) = file.as_mut() {
        let elapsed = START.get_or_init(Instant::now).elapsed().as_secs_f32();
        let _ = writeln!(f, "{elapsed:8.3}s {line}");
    }
}

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn debug_log_path() -> std::path::PathBuf {
    crate::settings::Settings::path().with_file_name("debug.log")
}

fn probe_kind(probe: &Probe) -> String {
    match probe {
        Probe::Password => "password field".into(),
        Probe::Text(t) => format!("text, {} chars", t.chars().count()),
        Probe::Unknown => "not exposed".into(),
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
    let target = imp::foreground();
    debug(
        "platform",
        format!("foreground app: {:?}", target.as_ref().map(|t| t.process.clone().unwrap_or_else(|| "?".into()))),
    );
    target
}

/// Brings `target` back to the front. Returns false if the system refused.
pub fn activate(target: &Target) -> bool {
    imp::activate(target)
}

pub fn probe_selection() -> Probe {
    let probe = imp::probe_selection();
    debug("platform", format!("UI Automation selection: {}", probe_kind(&probe)));
    probe
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
    debug("platform", format!("Ctrl+C copied: {:?} chars", copied.as_ref().map(|t| t.chars().count())));
    copied
}

/// Pastes `text` into `target` through the clipboard, then puts the old clipboard back.
pub fn paste(text: &str, target: Option<&Target>) -> Result<(), String> {
    if let Some(target) = target {
        let activated = activate(target);
        debug("platform", format!("brought {:?} back to the front: {activated}", target.process));
        if !activated {
            eprintln!("Grammable: could not bring the original window back to the front");
        }
        sleep(Duration::from_millis(120));
    }
    let saved = clipboard::save();
    if !clipboard::set_text(text, true) {
        return Err("Could not put the text on the clipboard.".into());
    }
    keys::paste()?;
    debug("platform", format!("Ctrl+V sent ({} chars)", text.chars().count()));
    // Give the app time to read the clipboard before restoring it.
    sleep(Duration::from_millis(450));
    // Only restore if nobody else has changed the clipboard in the meantime.
    if clipboard::get_text().as_deref() == Some(text) {
        clipboard::restore(saved);
    }
    Ok(())
}
