// Grammable — free, open-source AI writing assistant for Windows and Linux.
// Copyright (C) 2026 Udaya Lakmal
//
// This program is free software: you can redistribute it and/or modify it under the
// terms of the GNU General Public License as published by the Free Software Foundation,
// either version 3 of the License, or (at your option) any later version.
// This program is distributed WITHOUT ANY WARRANTY. See the LICENSE file for details.

// Release builds on Windows are GUI apps (no console window); the CLI re-attaches below.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod button;
mod guard;
mod hotkey;
mod monitor;
mod platform;
mod popup;
mod cli;
mod commands;
mod settings;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app_flag = |a: &String| a == app::ARG_BACKGROUND || a == app::ARG_POPUP;

    if args.first().is_some_and(|a| a.starts_with('-') && !app_flag(a)) {
        // Command-line mode, e.g.  grammable --run fix-grammar --backend ollama "teh text"
        attach_parent_console();
        std::process::exit(cli::run(&args));
    }

    app::run(&args);
}

/// A GUI-subsystem exe has no console; borrow the terminal it was started from so
/// `println!` shows up there. No-op when started from Explorer or on Linux.
fn attach_parent_console() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
