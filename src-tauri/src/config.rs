use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// Where the picker window appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Cursor,
    Center,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub show_tray: bool,
    pub placement: Placement,
    /// Browser ids in user-defined order. Browsers missing here go last.
    pub order: Vec<String>,
    /// Browser ids that should not be offered in the picker.
    pub hidden: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self { show_tray: true, placement: Placement::Cursor, order: Vec::new(), hidden: Vec::new() }
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("config.json"))
}

impl Config {
    /// Returns the stored config and whether it existed (false on first run).
    pub fn load(app: &AppHandle) -> (Self, bool) {
        let stored =
            path(app).and_then(|p| fs::read(p).ok()).and_then(|bytes| serde_json::from_slice(&bytes).ok());
        match stored {
            Some(config) => (config, true),
            None => (Self::default(), false),
        }
    }

    pub fn save(&self, app: &AppHandle) {
        let Some(path) = path(app) else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = fs::write(path, json);
        }
    }
}
