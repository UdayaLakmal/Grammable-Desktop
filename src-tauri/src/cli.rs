// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Scriptable interface, same commands as the macOS app — handy for testing
// presets and for shell workflows:
//
//     grammable --list
//     grammable --status
//     grammable --run <preset-id> [--backend <id>] [--diff] [text]   (text from stdin if omitted)

use std::io::{IsTerminal, Read};

use grammable_core::backends::check_availability;
use grammable_core::{run_preset, BackendId, PresetStore};

use crate::settings::Settings;

/// Exit code for bad usage (BSD `EX_USAGE`, same as the macOS app).
const USAGE: i32 = 64;

fn backend_list() -> String {
    BackendId::ALL.iter().map(|b| b.as_str()).collect::<Vec<_>>().join(", ")
}

fn usage() -> String {
    format!(
        "Grammable {} — free AI grammar checker & rewriter for Windows and Linux

Usage:
  grammable --list                                    List presets
  grammable --status                                  Show which AI backends are available
  grammable --run <preset> [--backend <id>] [--diff] [text]
                                                      Run a preset (reads stdin when no text is given)
  grammable --version
  grammable --help

Backends: {}
Settings: {}
Presets:  {}",
        env!("CARGO_PKG_VERSION"),
        backend_list(),
        Settings::path().display(),
        PresetStore::default_directory().display(),
    )
}

pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--list") => list(),
        Some("--status") => block_on(status()),
        Some("--run") => run_preset_command(&args[1..]),
        Some("--version" | "-V") => {
            println!("grammable {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help" | "-h") => {
            println!("{}", usage());
            0
        }
        _ => {
            eprintln!("{}", usage());
            USAGE
        }
    }
}

fn block_on<F: std::future::Future<Output = i32>>(f: F) -> i32 {
    match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt.block_on(f),
        Err(e) => {
            eprintln!("Error: could not start the async runtime: {e}");
            1
        }
    }
}

fn load_settings() -> Settings {
    let (settings, warning) = Settings::load();
    if let Some(w) = warning {
        eprintln!("Warning: {w}");
    }
    settings
}

fn list() -> i32 {
    let result = PresetStore::default().load();
    for p in &result.presets {
        println!("{:<18} {:<8} {}", p.id, p.mode.as_str(), p.name);
    }
    for e in &result.errors {
        eprintln!("Warning: {e}");
    }
    0
}

async fn status() -> i32 {
    let settings = load_settings();
    let checks = BackendId::ALL.map(|id| {
        let config = settings.backends.clone();
        tokio::spawn(async move { (id, check_availability(id, &config).await) })
    });
    for check in checks {
        if let Ok((id, status)) = check.await {
            let mark = if status.available { "✓" } else { "✗" };
            let default = if id == settings.default_backend { " (default)" } else { "" };
            println!("{mark} {:<18} {}{default}", id.as_str(), status.detail);
        }
    }
    0
}

fn run_preset_command(args: &[String]) -> i32 {
    let Some((preset_id, rest)) = args.split_first() else {
        eprintln!("{}", usage());
        return USAGE;
    };
    let preset_id = preset_id.to_lowercase();
    let settings = load_settings();

    let mut forced_backend = None;
    let mut show_diff = false;
    let mut words = Vec::new();
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--backend" => match rest.next().and_then(|b| BackendId::parse(b)) {
                Some(id) => forced_backend = Some(id),
                None => {
                    eprintln!("Unknown backend. Use one of: {}", backend_list());
                    return USAGE;
                }
            },
            "--diff" => show_diff = true,
            _ => words.push(arg.as_str()),
        }
    }

    let mut text = words.join(" ");
    if text.is_empty() {
        let stdin = std::io::stdin();
        if stdin.is_terminal() {
            eprintln!("Type or paste the text, then press {} on an empty line.", if cfg!(windows) { "Ctrl+Z, Enter" } else { "Ctrl+D" });
        }
        if let Err(e) = stdin.lock().read_to_string(&mut text) {
            eprintln!("Error: could not read stdin: {e}");
            return 1;
        }
    }
    if text.trim().is_empty() {
        eprintln!("No text given.");
        return USAGE;
    }

    let Some(mut preset) = PresetStore::default().load().presets.into_iter().find(|p| p.id == preset_id) else {
        eprintln!("No preset named “{preset_id}”. Run: grammable --list");
        return USAGE;
    };
    // --backend wins over the preset's own backend, like the macOS app.
    if let Some(id) = forced_backend {
        preset.backend = Some(id.as_str().into());
    }
    let default_backend = forced_backend.unwrap_or(settings.default_backend);

    block_on(async move {
        match run_preset(&preset, &text, default_backend, &settings.backends, settings.fallback_to_built_in).await {
            Ok(result) => {
                if show_diff {
                    let changes = result.diff.changes();
                    for c in &changes {
                        eprintln!("  “{}” → “{}”", c.original, c.replacement);
                    }
                    eprintln!(
                        "[{}, {:.1}s, {} changes]",
                        result.backend.display_name(),
                        result.duration_ms as f64 / 1000.0,
                        changes.len()
                    );
                }
                // Text piped in usually ends with a newline already; don't add a second one.
                if result.suggestion.ends_with('\n') {
                    print!("{}", result.suggestion);
                } else {
                    println!("{}", result.suggestion);
                }
                0
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        }
    })
}
