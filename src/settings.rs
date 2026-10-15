//! The player's settings, stored as `settings.json` in the data folder:
//! language, volume and whether rounds are recorded.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::t;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Language code (`en`, `sv`).
    pub language: String,
    /// Sound volume in percent, 0–100.
    pub volume: u8,
    /// Record every round automatically.
    pub recording: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: Lang::En.code().to_owned(),
            volume: 80,
            recording: true,
        }
    }
}

/// Volume changes in steps of this many percent.
pub const VOLUME_STEP: u8 = 10;

impl Settings {
    pub fn lang(&self) -> Lang {
        Lang::from_code(&self.language).unwrap_or_default()
    }

    /// Changes the volume by `steps` steps, staying within 0–100.
    pub fn change_volume(&mut self, steps: i32) {
        let v = i32::from(self.volume) + steps * i32::from(VOLUME_STEP);
        self.volume = v.clamp(0, 100) as u8;
    }
}

/// The program's data folder (overridable with `RUSTYHUSKY_DATA_DIR`).
pub fn data_dir() -> PathBuf {
    let env = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty());
    if let Some(dir) = env("RUSTYHUSKY_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(windows) {
        if let Some(appdata) = env("APPDATA") {
            return PathBuf::from(appdata).join("RustyHusky");
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = env("HOME") {
            return PathBuf::from(home).join("Library/Application Support/RustyHusky");
        }
    } else if let Some(data) = env("XDG_DATA_HOME") {
        return PathBuf::from(data).join("rustyhusky");
    } else if let Some(home) = env("HOME") {
        return PathBuf::from(home).join(".local/share/rustyhusky");
    }
    PathBuf::from("rustyhusky-data")
}

pub fn default_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// `RUSTYHUSKY_LANG`, which overrides the saved language for this run.
pub fn env_lang() -> Option<Lang> {
    std::env::var("RUSTYHUSKY_LANG")
        .ok()
        .and_then(|code| Lang::from_code(&code))
}

/// The saved settings. A missing or broken file gives the defaults, and
/// missing or invalid fields get their default values.
pub fn load(path: &Path) -> Settings {
    let mut s: Settings = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    s.volume = s.volume.min(100);
    if Lang::from_code(&s.language).is_none() {
        s.language = Settings::default().language;
    }
    s
}

/// Writes the settings via a temporary file, so a failed write never leaves
/// a half-written file.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).expect("settings serialize") + "\n";
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    };
    write().map_err(|e| t!("settings.save_failed", error = e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_saved_and_loaded() {
        let dir = crate::tests::temp_dir("settings");
        let path = dir.join("sub/settings.json");
        let defaults = load(&path);
        assert_eq!(defaults, Settings::default());
        assert!(defaults.recording, "recording is on by default");
        assert_eq!(defaults.lang(), Lang::En, "English by default");

        let mut s = Settings {
            language: "sv".into(),
            ..Settings::default()
        };
        s.change_volume(-3);
        s.recording = false;
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"volume\": 50"), "{text}");
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn broken_and_partial_files_fall_back_to_defaults() {
        let dir = crate::tests::temp_dir("settings-broken");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{ broken").unwrap();
        assert_eq!(load(&path), Settings::default());
        fs::write(&path, r#"{ "volume": 30 }"#).unwrap();
        let s = load(&path);
        assert_eq!((s.volume, s.recording, s.lang()), (30, true, Lang::En));
        fs::write(&path, r#"{ "volume": 300, "language": "xx" }"#).unwrap();
        // 300 does not fit a u8, so the whole file is ignored.
        assert_eq!(load(&path), Settings::default());
        fs::write(&path, r#"{ "volume": 200, "language": "xx" }"#).unwrap();
        let s = load(&path);
        assert_eq!((s.volume, s.language.as_str()), (100, "en"));
        // A folder that cannot be created gives an error, not a crash.
        fs::write(dir.join("file"), "x").unwrap();
        assert!(save(&dir.join("file/settings.json"), &s).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn volume_stays_in_range() {
        let mut s = Settings::default();
        s.change_volume(5);
        assert_eq!(s.volume, 100);
        s.change_volume(-20);
        assert_eq!(s.volume, 0);
    }
}
