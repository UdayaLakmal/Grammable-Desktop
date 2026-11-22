// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// X11: the active window comes from EWMH (_NET_ACTIVE_WINDOW, _NET_WM_PID),
// which every common window manager supports. X11 has no way to tell a
// password field apart, so terminals and password managers are excluded by name.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, KeyButMask, Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::CURRENT_TIME;

use super::{Probe, Target};

fn connect() -> Option<(RustConnection, Window)> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    Some((conn, root))
}

fn atom(conn: &RustConnection, name: &str) -> Option<Atom> {
    Some(conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom)
}

fn property32(conn: &RustConnection, window: Window, name: &str, kind: AtomEnum) -> Option<u32> {
    let atom = atom(conn, name)?;
    let reply = conn.get_property(false, window, atom, kind, 0, 1).ok()?.reply().ok()?;
    let value = reply.value32()?.next();
    value
}

pub fn foreground() -> Option<Target> {
    let (conn, root) = connect()?;
    let window = property32(&conn, root, "_NET_ACTIVE_WINDOW", AtomEnum::WINDOW).filter(|w| *w != 0)?;
    let pid = property32(&conn, window, "_NET_WM_PID", AtomEnum::CARDINAL);
    Some(Target { handle: window.into(), pid, process: pid.and_then(process_name) })
}

/// The executable's file name: /proc/<pid>/exe, else the (15-character) comm name.
fn process_name(pid: u32) -> Option<String> {
    if let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        if let Some(name) = exe.file_name() {
            return Some(name.to_string_lossy().trim_end_matches(" (deleted)").to_string());
        }
    }
    std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_string())
}

pub fn activate(target: &Target) -> bool {
    let Some((conn, root)) = connect() else { return false };
    let Some(active) = atom(&conn, "_NET_ACTIVE_WINDOW") else { return false };
    let Ok(window) = Window::try_from(target.handle) else { return false };
    // Source indication 2 ("pager") asks the window manager to honour the request.
    let event = ClientMessageEvent::new(32, window, active, [2, CURRENT_TIME, 0, 0, 0]);
    let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
    conn.send_event(false, root, mask, event).is_ok() && conn.flush().is_ok()
}

pub fn modifiers_held() -> bool {
    let Some((conn, root)) = connect() else { return false };
    let Some(reply) = conn.query_pointer(root).ok().and_then(|c| c.reply().ok()) else { return false };
    let held = KeyButMask::SHIFT | KeyButMask::CONTROL | KeyButMask::MOD1 | KeyButMask::MOD4;
    u16::from(reply.mask) & u16::from(held) != 0
}

/// The hotkey always copies on X11. PRIMARY holds the last selection made in
/// *any* app, so it can't tell whether the focused window still has one.
pub fn probe_selection() -> Probe {
    Probe::Unknown
}
