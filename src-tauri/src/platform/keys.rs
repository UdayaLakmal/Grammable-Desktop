// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Synthetic Ctrl+C / Ctrl+V for the window in front.

use enigo::Direction::{Click, Press, Release};
use enigo::{Enigo, Key, Keyboard, Settings};

pub fn copy() -> Result<(), String> {
    ctrl(letter('c', 0x43))
}

pub fn paste() -> Result<(), String> {
    ctrl(letter('v', 0x56))
}

/// Windows: the virtual-key code, which works with any keyboard layout.
#[cfg(windows)]
fn letter(_c: char, vk: u32) -> Key {
    Key::Other(vk)
}

#[cfg(not(windows))]
fn letter(c: char, _vk: u32) -> Key {
    Key::Unicode(c)
}

fn ctrl(key: Key) -> Result<(), String> {
    let err = |e: &dyn std::fmt::Display| format!("Could not send keystrokes: {e}");
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| err(&e))?;
    enigo.key(Key::Control, Press).map_err(|e| err(&e))?;
    let result = enigo.key(key, Click).map_err(|e| err(&e));
    enigo.key(Key::Control, Release).map_err(|e| err(&e))?;
    result
}
