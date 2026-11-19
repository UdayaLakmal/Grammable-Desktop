# Implementation notes (temporary)

Working notes for the Windows / Linux port of [Grammable for macOS](https://github.com/UdayaLakmal/Grammable).
Delete this file once the app ships.

## Status — 2026-11-19

Core crate and command-line mode build and pass their tests on Ubuntu (Rust 1.100).
Nothing has been built on Windows yet.

| Part | State |
|---|---|
| `crates/grammable-core` — presets, prompt, diff, engine, backends | Ported from the Swift `GrammableCore`. **Builds, 31 tests pass** on Ubuntu. |
| `crates/grammable-core/src/backends/builtin.rs` — Harper offline checker | Works. Slow (~7 s per check in a debug build) and over-eager: it applies style lints too (e.g. "stdin" → "standard input"). Restrict it to spelling/grammar lints. |
| `src-tauri/` — CLI mode | **Done:** `--list`, `--status`, `--run <preset> [--backend <id>] [--diff] [text]`, `--version`, `--help` (same as macOS). Settings in `settings.rs`. Windows `AttachConsole` not yet compiled on Windows. |
| `src-tauri/` — the desktop app | No Tauri yet; running without arguments prints a notice. |
| `ui/` — popup, review, settings | Not started. |
| `.github/workflows/` — CI, release, winget | Not started. |
| App icons | Only `src-tauri/icons/app-icon-1024.png` (copied from the macOS app). Generate the rest with `npx tauri icon`. |

## Decisions

- **Stack:** Tauri 2 (Rust + plain HTML/JS UI, no frontend build step; use `withGlobalTauri`).
- **Repo layout:** Cargo workspace — `crates/grammable-core` (pure logic, no UI) + `src-tauri` (app, binary `grammable`).
- **Presets:** same Markdown front-matter format as macOS, same folder `~/.grammable/presets/`. `presets/` is a copy of the macOS repo's folder, and a core test parses every file in it.
- **Backends:** Claude Code, Codex, Gemini, Antigravity, Devin CLIs; Ollama; OpenAI-compatible (LM Studio etc.); `builtin` (Harper) replaces the macOS spell checker. Apple Intelligence is dropped.
- **Windows `.cmd` shims:** npm installs CLIs as batch files, which can't take multi-line arguments. `shell_env::is_batch` detects them; Claude then sends the system prompt over stdin, Gemini reads the prompt from stdin. `.exe` is preferred over `.cmd` when both exist.
- **Fallback:** a `fix` preset falls back to the built-in checker when the chosen AI isn't installed or running (same as macOS).
- **Bundle id:** `io.github.udayalakmal.grammable`. **winget id:** `UdayaLakmal.Grammable`.

## Ubuntu setup

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install build-essential pkg-config cmake libssl-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libxdo-dev
cargo test --workspace
```

Use `libayatana-appindicator3-dev`, not `libappindicator3-dev`: on current Ubuntu the latter removes
`update-notifier`. The first build is slow — harper-core pulls in `burn`, `candle` and `wgpu`.

## Next steps

1. ~~Get `grammable-core` green~~ — done.
2. ~~CLI mode~~ — done (`src-tauri/src/cli.rs`). Settings file: `dirs::config_dir()/Grammable/settings.json` (`defaultBackend`, `fallbackToBuiltIn`, `backends`); the app adds its own fields to the same struct.
3. **Tauri app** (`tauri`, `tauri-plugin-global-shortcut`, `tauri-plugin-autostart`, `tauri-plugin-opener`, `arboard`, `enigo`):
   - Tray menu: Enabled ✓ · Check clipboard text · Settings… · Open presets folder · Quit. Closing windows must not quit the app.
   - Global shortcut `Ctrl+Alt+G` (configurable; AltGr clashes on some layouts). Act on key **release** so Ctrl+Alt aren't still held when we send Ctrl+C.
   - `selection.rs`:
     - Linux X11: read the PRIMARY selection first (`arboard` `LinuxClipboardKind::Primary`), no keystroke needed.
     - Fallback (Windows / Linux): save clipboard → Ctrl+C via `enigo` → poll ≤400 ms → restore clipboard.
     - Windows: remember `GetForegroundWindow()` and `SetForegroundWindow()` it before pasting.
     - Replace: put text on clipboard → Ctrl+V → restore the old clipboard after ~250 ms.
     - Wayland (`XDG_SESSION_TYPE=wayland`): no keystroke injection or window positioning; offer **Copy** instead of Replace.
   - Popup window (`label: popup`, undecorated, always on top, skip taskbar, hidden by default) shown at the cursor; hides on blur.
   - Commands: `get_popup_state`, `run_preset`, `replace_text`, `copy_text`, `hide_popup`, `get_settings`, `save_settings`, `backend_statuses`, `open_presets_folder`.
   - Settings: extend `settings::Settings` (already has default backend, fallback to built-in, `BackendConfig`) with hotkey, start at login, excluded apps (process names), plus `save()`.
4. **UI** (`ui/`): action menu → loading → review. Fix mode: accept/reject each change (`TextDiff` segments). Rewrite mode: editable before/after. On-device / Cloud badge. Map SF Symbol icon names to emoji.
5. **Later:** Windows UI Automation for reading the selection without the clipboard, password-field guard (UIA `IsPassword`, AT-SPI on Linux), floating "G" button, Ollama one-click setup (`winget install Ollama.Ollama` / `ollama.com/install.sh`).

## Release & deploy plan

- **CI** (`ci.yml`): matrix `ubuntu-22.04` + `windows-latest`; `cargo test -p grammable-core`; `cargo build`.
- **Release** (`release.yml`, on tag `v*`): `tauri-apps/tauri-action` → GitHub Release with
  Windows NSIS `.exe` (per-user, silent `/S`) + `.msi`, Linux AppImage + `.deb` + `.rpm`. Build Linux on `ubuntu-22.04` for older glibc.
- **winget:**
  - Fork of `microsoft/winget-pkgs` and the `WINGET_TOKEN` secret (classic PAT, `public_repo`) — **done**.
  - First version must be submitted by hand (e.g. with Komac) after the first GitHub Release; Microsoft reviews it.
  - Afterwards `winget-releaser` in `release.yml` opens the update PR automatically (installer regex `_x64-setup\.exe$`). Gate it behind a repo variable until the first submission is merged.
- **Signing:** apply to SignPath Foundation (free for OSS). Ship unsigned until approved; add the signing step then. README needs a "Code signing policy" section.
- **Auto-update (optional):** `tauri-plugin-updater` + `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` secrets.

## Known limitations to document in the README

- Wayland: no auto-popup button, hotkey only through the XDG GlobalShortcuts portal, Replace needs `ydotool`/`wtype`, otherwise Copy only.
- Windows: elevated (admin) windows can't be read or typed into; unsigned builds trigger SmartScreen.
- No Apple Intelligence; the built-in checker (Harper) is English-only.
