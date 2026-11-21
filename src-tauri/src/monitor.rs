// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Watches global mouse and keyboard input (Windows: low-level hooks; X11: the
// RECORD extension, via `rdev`) to notice fresh selections for the G button,
// and to hide the button when the user does anything else. Only *which* keys
// are pressed is looked at, never what is typed. Not available on Wayland.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rdev::{Button as Mouse, EventType, Key};
use tauri::{AppHandle, Manager};

use crate::app::AppState;
use crate::button::{self, Pending};
use crate::guard::{self, Verdict};
use crate::platform::{self, Probe};
use crate::popup;

/// Grammable's own Ctrl+C / Ctrl+V are ignored until this time (ms since the epoch).
static SYNTHETIC_UNTIL: AtomicU64 = AtomicU64::new(0);
/// Debounces selection checks: only the latest one runs.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// How long after the selection to read it: lets the app finish updating it.
const SETTLE: Duration = Duration::from_millis(180);
const DRAG_MIN: f64 = 5.0;
const DOUBLE_CLICK: Duration = Duration::from_millis(450);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Call before sending keystrokes, so the monitor doesn't react to them.
pub fn mark_synthetic() {
    SYNTHETIC_UNTIL.store(now_ms() + 400, Ordering::SeqCst);
}

pub fn start(app: AppHandle) {
    #[cfg(target_os = "linux")]
    if platform::wayland::is_wayland() {
        return;
    }
    std::thread::Builder::new()
        .name("grammable-input".into())
        .spawn(move || {
            let mut watcher = Watcher::default();
            if let Err(e) = rdev::listen(move |event| watcher.handle(&app, event.event_type)) {
                eprintln!("Grammable: can't watch for selections, so the G button is off: {e:?}");
            }
        })
        .ok();
}

#[derive(Default)]
struct Watcher {
    mouse: (f64, f64),
    /// Where and when the left button went down, unless it was on the G button.
    down: Option<((f64, f64), Instant)>,
    last_click: Option<((f64, f64), Instant)>,
    clicks: u32,
    shift: bool,
    ctrl: bool,
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

impl Watcher {
    fn handle(&mut self, app: &AppHandle, event: EventType) {
        if now_ms() < SYNTHETIC_UNTIL.load(Ordering::SeqCst) {
            return;
        }
        match event {
            EventType::MouseMove { x, y } => self.mouse = (x, y),
            EventType::ButtonPress(button) => {
                let on_button = button::contains(app, self.mouse.0, self.mouse.1);
                if !on_button {
                    // Clicking anywhere else means the user moved on.
                    button::hide(app);
                }
                if button == Mouse::Left && !on_button {
                    self.down = Some((self.mouse, Instant::now()));
                } else {
                    self.down = None;
                }
            }
            EventType::ButtonRelease(Mouse::Left) => {
                let Some((start, _)) = self.down.take() else { return };
                if distance(start, self.mouse) >= DRAG_MIN {
                    self.clicks = 0;
                    schedule(app);
                    return;
                }
                let now = Instant::now();
                let repeated = self
                    .last_click
                    .is_some_and(|(p, t)| now.duration_since(t) < DOUBLE_CLICK && distance(p, self.mouse) < DRAG_MIN);
                self.clicks = if repeated { self.clicks + 1 } else { 1 };
                self.last_click = Some((self.mouse, now));
                if self.clicks >= 2 {
                    schedule(app); // double click selects a word, triple a line
                }
            }
            EventType::Wheel { .. } => button::hide(app),
            EventType::KeyPress(key) => {
                button::hide(app);
                match key {
                    Key::ShiftLeft | Key::ShiftRight => self.shift = true,
                    Key::ControlLeft | Key::ControlRight => self.ctrl = true,
                    Key::LeftArrow | Key::RightArrow | Key::UpArrow | Key::DownArrow | Key::Home | Key::End
                    | Key::PageUp | Key::PageDown
                        if self.shift =>
                    {
                        schedule(app)
                    }
                    Key::KeyA if self.ctrl => schedule(app),
                    _ => {}
                }
            }
            EventType::KeyRelease(key) => match key {
                Key::ShiftLeft | Key::ShiftRight => self.shift = false,
                Key::ControlLeft | Key::ControlRight => self.ctrl = false,
                _ => {}
            },
            _ => {}
        }
    }
}

/// Checks the selection shortly, unless something newer comes first.
fn schedule(app: &AppHandle) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SETTLE);
        if GENERATION.load(Ordering::SeqCst) == generation {
            check(&app);
        }
    });
}

fn check(app: &AppHandle) {
    let settings = app.state::<AppState>().settings();
    if !settings.is_enabled || !settings.popup_on_selection || popup::is_visible(app) {
        return;
    }
    let target = platform::foreground();
    if guard::check_target(target.as_ref(), &settings) != Verdict::Ok {
        return;
    }
    let text = match platform::probe_selection() {
        Probe::Password => return,
        Probe::Text(text) => Some(text),
        // X11: the selection just made is in PRIMARY. Windows: copy on click.
        Probe::Unknown => platform::read_primary(),
    };
    if let Some(text) = &text {
        if guard::check_text(Some(text), &settings) != Verdict::Ok {
            return;
        }
    }
    let Ok(mouse) = app.cursor_position() else { return };
    button::show(app, Pending { text, target, anchor: (mouse.x, mouse.y) });
}
