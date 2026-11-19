// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Pure logic shared by the Windows and Linux app and the command-line mode:
// presets, prompts, diffing and the AI backends. No UI, no OS-specific code
// except finding and running command-line tools.

pub mod backends;
pub mod diff;
pub mod engine;
pub mod presets;
pub mod prompt;
pub mod shell_env;

pub use backends::{AiRequest, BackendConfig, BackendError, BackendId, BackendStatus};
pub use diff::{Segment, TextChange, TextDiff};
pub use engine::{run as run_preset, EngineResult};
pub use presets::{Preset, PresetMode, PresetStore};
