//! User settings, persisted next to the identity in the app-data directory.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "settings.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// When set, senders must pass `?pin=` on `prepare-upload`.
    pub pin: Option<String>,
    /// Accept incoming requests without asking. Off by default on purpose.
    pub quick_save: bool,
}

impl Settings {
    pub fn file_path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    /// Missing or unreadable settings fall back to the defaults: a broken
    /// file must not stop the app from starting, and the defaults are the
    /// safe end of every option.
    pub fn load(dir: &Path) -> Settings {
        let path = Settings::file_path(dir);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                eprintln!("settings unreadable ({e}); using defaults");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE_NAME}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, Settings::file_path(dir))
    }

    /// An empty PIN in the file means "no PIN", not "PIN is the empty string".
    pub fn required_pin(&self) -> Option<&str> {
        self.pin.as_deref().filter(|pin| !pin.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toss-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_are_the_safe_end() {
        let settings = Settings::default();
        assert!(!settings.quick_save);
        assert_eq!(settings.required_pin(), None);
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let dir = temp_dir();
        let settings = Settings {
            pin: Some("123456".into()),
            quick_save: true,
        };
        settings.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir), settings);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = temp_dir();
        assert_eq!(Settings::load(&dir), Settings::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = temp_dir();
        std::fs::write(Settings::file_path(&dir), b"{ not json").unwrap();
        assert_eq!(Settings::load(&dir), Settings::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_pin_means_no_pin() {
        let settings = Settings {
            pin: Some(String::new()),
            quick_save: false,
        };
        assert_eq!(settings.required_pin(), None);
    }

    #[test]
    fn partial_json_keeps_other_defaults() {
        let dir = temp_dir();
        std::fs::write(Settings::file_path(&dir), br#"{"quickSave":true}"#).unwrap();
        let loaded = Settings::load(&dir);
        assert!(loaded.quick_save);
        assert_eq!(loaded.pin, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
