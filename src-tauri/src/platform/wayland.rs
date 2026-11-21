// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Wayland, through the XDG desktop portals (GNOME 46+, KDE Plasma 6):
//
//   RemoteDesktop  — types Ctrl+C / Ctrl+V into the focused app. The desktop
//                    asks the user once; the answer is remembered with a
//                    restore token, saved next to settings.json.
//   Clipboard      — reads and writes the clipboard through that same session,
//                    without Grammable's own window having focus.
//
// Wayland apps can't see which window is in front or where the mouse is, so
// there are no excluded-app checks or popup placement here.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ashpd::desktop::clipboard::{Clipboard, RequestClipboardOptions, SetSelectionOptions};
use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions};
use ashpd::desktop::{PersistMode, Session};
use futures_util::StreamExt;

pub const APP_ID: &str = "io.github.udayalakmal.grammable";

// Ctrl+Insert / Shift+Insert copy and paste in GTK, Qt, Firefox, Chromium,
// Electron and LibreOffice. Unlike Ctrl+C they don't interrupt a program
// running in a terminal, and Wayland gives no way to see that a terminal is in front.
const KEY_CONTROL_L: i32 = 0xffe3;
const KEY_SHIFT_L: i32 = 0xffe1;
const KEY_INSERT: i32 = 0xff63;
const TEXT_TYPES: &[&str] = &["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "STRING", "TEXT"];

struct Portal {
    remote: RemoteDesktop,
    clipboard: Option<Clipboard>,
    session: Session<RemoteDesktop>,
}

fn portal_slot() -> &'static tokio::sync::Mutex<Option<Arc<Portal>>> {
    static SLOT: OnceLock<tokio::sync::Mutex<Option<Arc<Portal>>>> = OnceLock::new();
    SLOT.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// What the portal last told us about the clipboard: a change counter, whether
/// Grammable owns it, and the formats offered. GNOME only allows reading the
/// formats announced here.
#[derive(Default, Clone)]
struct Owner {
    changes: u64,
    ours: bool,
    mime_types: Vec<String>,
}
static OWNER: Mutex<Option<Owner>> = Mutex::new(None);

fn owner() -> Owner {
    OWNER.lock().unwrap().clone().unwrap_or_default()
}

/// Text Grammable is currently offering on the clipboard.
static OFFERED: Mutex<Option<String>> = Mutex::new(None);

fn token_path() -> PathBuf {
    crate::settings::Settings::path().with_file_name("portal-token")
}

pub fn has_saved_consent() -> bool {
    token_path().exists()
}

/// True when running under Wayland (Grammable's own window may still be X11 via GDK_BACKEND).
pub fn is_wayland() -> bool {
    crate::commands::session_type() == "wayland"
}

/// The running portal session, starting one if needed. Without a saved token
/// this shows the desktop's "Allow remote interaction?" dialog and waits for it.
async fn portal() -> Result<Arc<Portal>, String> {
    let mut slot = portal_slot().lock().await;
    if let Some(p) = slot.as_ref() {
        return Ok(p.clone());
    }
    let p = Arc::new(start().await?);
    serve_clipboard(p.clone());
    watch_owner(p.clone());
    *slot = Some(p.clone());
    Ok(p)
}

/// Tells the portals which app this is, so GNOME remembers the user's answers
/// and lists Grammable by name. Must come before any other portal call.
async fn register_app() {
    static DONE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    DONE.get_or_init(|| async {
        ensure_desktop_entry();
        if let Ok(id) = ashpd::AppID::try_from(APP_ID) {
            if let Err(e) = ashpd::register_host_app(id).await {
                eprintln!("Grammable: portal app registration failed: {e}");
            }
        }
    })
    .await;
}

async fn start() -> Result<Portal, String> {
    register_app().await;
    let err = |what: &str, e: ashpd::Error| format!("{what}: {e}");
    let remote = RemoteDesktop::new().await.map_err(|e| err("Remote desktop portal unavailable", e))?;
    let session = remote.create_session(Default::default()).await.map_err(|e| err("Could not start a portal session", e))?;

    // The clipboard has to be requested before the session starts.
    let clipboard = match Clipboard::new().await {
        Ok(c) => c.request(&session, RequestClipboardOptions::default()).await.ok().map(|_| c),
        Err(_) => None,
    };

    let token = std::fs::read_to_string(token_path()).ok();
    remote
        .select_devices(
            &session,
            SelectDevicesOptions::default()
                .set_devices(DeviceType::KEYBOARD)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(token.as_deref().map(str::trim)),
        )
        .await
        .map_err(|e| err("Could not select the keyboard", e))?;
    let started = remote
        .start(&session, None, Default::default())
        .await
        .map_err(|e| err("Could not start the session", e))?
        .response()
        .map_err(|e| match e {
            ashpd::Error::Response(_) => "Keyboard access was not allowed.".to_string(),
            e => err("Could not start the session", e),
        })?;
    if let Some(token) = started.restore_token() {
        if let Some(dir) = token_path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(token_path(), token);
    }
    let clipboard = clipboard.filter(|_| started.is_clipboard_enabled());
    if clipboard.is_none() {
        eprintln!("Grammable: the portal did not grant clipboard access");
    }
    Ok(Portal { remote, clipboard, session })
}

fn watch_owner(portal: Arc<Portal>) {
    tokio::spawn(async move {
        let Some(clipboard) = portal.clipboard.as_ref() else { return };
        let Ok(changes) = clipboard.receive_selection_owner_changed::<RemoteDesktop>().await else { return };
        let mut changes = Box::pin(changes);
        while let Some((_session, change)) = changes.next().await {
            debug(format!("owner changed: ours={:?} types={:?}", change.session_is_owner(), change.mime_types()));
            let mut slot = OWNER.lock().unwrap();
            let previous = slot.take().unwrap_or_default();
            *slot = Some(Owner {
                changes: previous.changes + 1,
                ours: change.session_is_owner().unwrap_or(false),
                mime_types: change.mime_types().to_vec(),
            });
        }
    });
}

/// Answers paste requests for text Grammable put on the clipboard.
fn serve_clipboard(portal: Arc<Portal>) {
    tokio::spawn(async move {
        let Some(clipboard) = portal.clipboard.as_ref() else { return };
        let Ok(transfers) = clipboard.receive_selection_transfer::<RemoteDesktop>().await else { return };
        let mut transfers = Box::pin(transfers);
        while let Some((_session, mime, serial)) = transfers.next().await {
            debug(format!("paste request for {mime}"));
            let text = OFFERED.lock().unwrap().clone().unwrap_or_default();
            let ok = match clipboard.selection_write(&portal.session, serial).await {
                Ok(fd) => {
                    let mut file = std::fs::File::from(blocking(std::os::fd::OwnedFd::from(fd)));
                    file.write_all(text.as_bytes()).is_ok()
                }
                Err(_) => false,
            };
            let _ = clipboard.selection_write_done(&portal.session, serial, ok).await;
        }
    });
}

/// Starts the session now, e.g. from Settings, so the consent dialog appears at a sensible moment.
pub async fn connect() -> Result<(), String> {
    portal().await.map(|_| ())
}

/// Whether a session is running right now.
pub async fn is_connected() -> bool {
    portal_slot().lock().await.is_some()
}

/// Forgets the session and the saved consent.
pub async fn disconnect() {
    if let Some(p) = portal_slot().lock().await.take() {
        let _ = p.session.close().await;
    }
    let _ = std::fs::remove_file(token_path());
}

async fn chord(modifier: i32, key: i32) -> Result<(), String> {
    let p = portal().await?;
    let send = |sym, state| p.remote.notify_keyboard_keysym(&p.session, sym, state, Default::default());
    let result = async {
        send(modifier, KeyState::Pressed).await?;
        send(key, KeyState::Pressed).await?;
        send(key, KeyState::Released).await?;
        send(modifier, KeyState::Released).await
    }
    .await;
    if result.is_err() {
        // The session may have been closed by the desktop; start over next time.
        portal_slot().lock().await.take();
    }
    result.map_err(|e| format!("Could not send keystrokes: {e}"))
}

/// The portal hands out non-blocking pipes; reading them on a worker thread is simpler blocking.
fn blocking(fd: std::os::fd::OwnedFd) -> std::os::fd::OwnedFd {
    use std::os::fd::AsRawFd;
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK);
        }
    }
    fd
}

fn debug(message: impl std::fmt::Display) {
    if std::env::var_os("GRAMMABLE_DEBUG").is_some() {
        eprintln!("[wayland] {message}");
    }
}

/// The clipboard's text. Grammable can't read its own offer through the
/// portal, so that comes from memory.
async fn read_clipboard() -> Option<String> {
    let p = portal().await.ok()?;
    let clipboard = p.clipboard.as_ref()?;
    let owner = owner();
    if owner.ours {
        return OFFERED.lock().unwrap().clone();
    }
    // Prefer the formats the owner announced, in our order of preference.
    let mut types: Vec<&str> = TEXT_TYPES.iter().copied().filter(|t| owner.mime_types.iter().any(|m| m == t)).collect();
    if types.is_empty() {
        types = TEXT_TYPES.to_vec();
    }
    for mime in types {
        let fd = match clipboard.selection_read(&p.session, mime).await {
            Ok(fd) => fd,
            Err(e) => {
                debug(format!("read {mime}: {e}"));
                continue;
            }
        };
        let file = std::fs::File::from(blocking(std::os::fd::OwnedFd::from(fd)));
        let read = tokio::task::spawn_blocking(move || {
            let mut bytes = Vec::new();
            let mut file = file;
            file.read_to_end(&mut bytes).map(|_| bytes)
        });
        match tokio::time::timeout(Duration::from_millis(400), read).await {
            Ok(Ok(Ok(bytes))) => {
                debug(format!("read {mime}: {} bytes", bytes.len()));
                return Some(String::from_utf8_lossy(&bytes).into_owned());
            }
            Ok(Ok(Err(e))) => debug(format!("read {mime}: {e}")),
            Ok(Err(e)) => debug(format!("read {mime}: {e}")),
            Err(_) => debug(format!("read {mime}: timed out")),
        }
    }
    None
}

async fn write_clipboard(text: &str) -> Result<(), String> {
    let p = portal().await?;
    let clipboard = p.clipboard.as_ref().ok_or("The desktop did not allow clipboard access.")?;
    *OFFERED.lock().unwrap() = Some(text.to_string());
    clipboard
        .set_selection(&p.session, SetSelectionOptions::default().set_mime_types(TEXT_TYPES))
        .await
        .map_err(|e| format!("Could not write the clipboard: {e}"))
}

/// Diagnostics for `--portal-test`: can we read and write the clipboard at all?
pub async fn self_test() -> Vec<String> {
    let mut out = Vec::new();
    let p = match portal().await {
        Ok(p) => p,
        Err(e) => return vec![format!("✗ {e}")],
    };
    out.push(format!("clipboard granted: {}", p.clipboard.is_some()));
    out.push(format!("clipboard now: {:?}", read_clipboard().await.map(|t| t.chars().take(40).collect::<String>())));
    match write_clipboard("grammable self-test").await {
        Ok(()) => {
            tokio::time::sleep(Duration::from_millis(100)).await;
            out.push(format!("after write, reads back: {:?}", read_clipboard().await));
        }
        Err(e) => out.push(format!("✗ write failed: {e}")),
    }
    out
}

/// Waits up to `limit` for another app to take the clipboard after change number `since`.
async fn wait_for_foreign_owner(since: u64, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        let o = owner();
        if o.changes > since && !o.ours {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// Copies the focused app's selection and returns it. The clipboard is restored (text only).
pub async fn copy_selection() -> Result<Option<String>, String> {
    portal().await?;
    let saved = read_clipboard().await;
    let before = owner().changes;
    chord(KEY_CONTROL_L, KEY_INSERT).await?;
    if !wait_for_foreign_owner(before, Duration::from_millis(600)).await {
        return Ok(None); // nothing selected, or the app ignored the keys
    }
    let copied = read_clipboard().await;
    if let Some(saved) = saved {
        let _ = write_clipboard(&saved).await;
    }
    Ok(copied)
}

/// Puts `text` on the clipboard, pastes it into the focused app, then restores the clipboard.
pub async fn paste(text: &str) -> Result<(), String> {
    let saved = read_clipboard().await;
    write_clipboard(text).await?;
    tokio::time::sleep(Duration::from_millis(60)).await;
    let ours = owner().changes;
    chord(KEY_SHIFT_L, KEY_INSERT).await?;
    tokio::time::sleep(Duration::from_millis(450)).await;
    // Restore only if nobody has put something else on the clipboard meanwhile.
    if owner().changes == ours {
        if let Some(saved) = saved {
            let _ = write_clipboard(&saved).await;
        }
    }
    Ok(())
}

/// Copies text the normal way (the user asked for it to stay on the clipboard).
pub async fn copy_text(text: &str) -> Result<(), String> {
    write_clipboard(text).await
}

/// Portals identify host apps by their .desktop file. Installed packages ship
/// one; for AppImages and development builds, write one to ~/.local/share.
fn ensure_desktop_entry() {
    let name = format!("{APP_ID}.desktop");
    let mut dirs: Vec<PathBuf> = dirs::data_dir().into_iter().collect();
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    if dirs.iter().any(|d| d.join("applications").join(&name).exists()) {
        return;
    }
    let (Some(data), Ok(exe)) = (dirs::data_dir(), std::env::current_exe()) else { return };
    let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).unwrap_or(exe);
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Grammable\nComment=AI writing assistant\n\
         Exec=\"{}\" %U\nIcon=grammable\nTerminal=false\nCategories=Utility;TextTools;\nNoDisplay=true\n",
        exe.display()
    );
    let dir = data.join("applications");
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(&name), entry);
    }
}

// MARK: Global shortcut

const SHORTCUT_ID: &str = "improve-selection";

/// "Ctrl+Alt+G" → "CTRL+ALT+g", the XDG shortcut syntax.
pub fn portal_trigger(accelerator: &str) -> String {
    accelerator
        .split('+')
        .map(|part| {
            let part = part.trim();
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "commandorcontrol" | "cmdorctrl" => "CTRL".to_string(),
                "alt" | "option" => "ALT".to_string(),
                "shift" => "SHIFT".to_string(),
                "super" | "meta" | "win" | "cmd" | "command" | "logo" => "LOGO".to_string(),
                "space" => "space".to_string(),
                "enter" | "return" => "Return".to_string(),
                _ if part.chars().count() == 1 => part.to_lowercase(),
                _ => part.to_string(), // key names like F2 keep their case
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Binds the shortcut through the GlobalShortcuts portal. The first time, GNOME
/// asks the user to confirm it; after that the desktop owns it (Settings › Apps
/// › Grammable). Returns the trigger as the desktop describes it.
/// `on_activate` gets the activation token the desktop passes, if any.
pub async fn bind_shortcut(
    preferred: &str,
    on_activate: impl Fn(Option<String>) + Send + 'static,
) -> Result<String, String> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};

    register_app().await;
    let err = |e: ashpd::Error| format!("The desktop's shortcut service is unavailable: {e}");
    let portal = GlobalShortcuts::new().await.map_err(err)?;
    let session = portal.create_session(Default::default()).await.map_err(err)?;
    let trigger = portal_trigger(preferred);
    let shortcut = NewShortcut::new(SHORTCUT_ID, "Improve the selected text").preferred_trigger(Some(trigger.as_str()));
    let bound = portal
        .bind_shortcuts(&session, &[shortcut], None, Default::default())
        .await
        .map_err(err)?
        .response()
        .map_err(|_| "The shortcut was not allowed.".to_string())?;
    let description = bound
        .shortcuts()
        .iter()
        .find(|s| s.id() == SHORTCUT_ID)
        .map(|s| s.trigger_description().to_string())
        .unwrap_or_default();
    let activations = portal.receive_activated().await.map_err(err)?;
    tokio::spawn(async move {
        let _keep = (&portal, &session); // the binding lives as long as the session
        let mut activations = Box::pin(activations);
        while let Some(event) = activations.next().await {
            if event.shortcut_id() == SHORTCUT_ID {
                let token = event
                    .options()
                    .get("activation_token")
                    .and_then(|v| String::try_from(v.clone()).ok());
                on_activate(token);
            }
        }
    });
    Ok(description)
}

#[cfg(test)]
mod tests {
    #[test]
    fn converts_accelerators() {
        assert_eq!(super::portal_trigger("Ctrl+Alt+G"), "CTRL+ALT+g");
        assert_eq!(super::portal_trigger("Super+Shift+F2"), "LOGO+SHIFT+F2");
    }
}
