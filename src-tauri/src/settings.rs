use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub volume: f32,
    pub muted: bool,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub profile: String,
    /// Output device name; `None` follows the system default.
    pub output_device: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 0.7,
            muted: false,
            close_to_tray: true,
            start_minimized: false,
            profile: crate::audio::DEFAULT_PROFILE.to_string(),
            output_device: None,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if let Err(err) = std::fs::write(path, json) {
                eprintln!("clicky: couldn't save settings: {err}");
            }
        }
    }
}
