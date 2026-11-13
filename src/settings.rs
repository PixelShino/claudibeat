//! The few choices claudibeat remembers, in `%APPDATA%\claudibeat\settings.json`.

use crate::text::Lang;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Play a soft sound with each toast.
    pub sound: bool,
    /// The interface language; English unless the person picks Russian.
    pub language: Lang,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { sound: true, language: Lang::En }
    }
}

impl Settings {
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
        base.join("claudibeat").join("settings.json")
    }

    /// The saved settings, or the defaults when the file is missing or torn.
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}
