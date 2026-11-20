// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// One long-lived clipboard handle. On X11 the clipboard owner has to stay
// alive to serve pastes, so it is never dropped.
//
// "Transient" writes (Grammable's own temporary use of the clipboard) are
// marked so clipboard history ignores them: Windows' Win+V history and cloud
// sync, and KDE/GNOME clipboard managers on Linux.

use std::sync::Mutex;

use arboard::{Clipboard, ImageData};

static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Clipboard) -> R) -> Option<R> {
    let mut guard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        match Clipboard::new() {
            Ok(c) => *guard = Some(c),
            Err(e) => {
                eprintln!("Grammable: clipboard unavailable: {e}");
                return None;
            }
        }
    }
    guard.as_mut().map(f)
}

/// The clipboard as it was before Grammable touched it.
pub enum Saved {
    Text(String),
    Image(ImageData<'static>),
    Empty,
}

pub fn save() -> Saved {
    with(|c| {
        if let Ok(text) = c.get_text() {
            return Saved::Text(text);
        }
        match c.get_image() {
            Ok(image) => Saved::Image(image.to_owned_img()),
            Err(_) => Saved::Empty,
        }
    })
    .unwrap_or(Saved::Empty)
}

pub fn restore(saved: Saved) {
    with(|c| {
        let result = match saved {
            Saved::Text(text) => transient(c.set()).text(text),
            Saved::Image(image) => transient(c.set()).image(image),
            Saved::Empty => c.clear(),
        };
        if let Err(e) = result {
            eprintln!("Grammable: could not restore the clipboard: {e}");
        }
    });
}

pub fn get_text() -> Option<String> {
    with(|c| c.get_text().ok()).flatten()
}

/// Puts text on the clipboard. `transient` keeps it out of clipboard history.
pub fn set_text(text: &str, transient_write: bool) -> bool {
    with(|c| {
        let result = if transient_write { transient(c.set()).text(text) } else { c.set_text(text) };
        result.map_err(|e| eprintln!("Grammable: could not write the clipboard: {e}")).is_ok()
    })
    .unwrap_or(false)
}

#[cfg(windows)]
fn transient(set: arboard::Set<'_>) -> arboard::Set<'_> {
    use arboard::SetExtWindows;
    set.exclude_from_monitoring()
}

#[cfg(target_os = "linux")]
fn transient(set: arboard::Set<'_>) -> arboard::Set<'_> {
    use arboard::SetExtLinux;
    set.exclude_from_history()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn transient(set: arboard::Set<'_>) -> arboard::Set<'_> {
    set
}
