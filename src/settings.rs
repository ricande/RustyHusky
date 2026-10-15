//! The player's settings, stored as `settings.json` in the data folder:
//! language, volume and the recording mode.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{self, APP_ID, APP_NAME};
use crate::i18n::Lang;
use crate::t;

/// How rounds are recorded. Takes effect from the next round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingMode {
    /// The game follows real time; the video keeps up or recording stops.
    Gameplay,
    /// Every video frame is produced from exactly two physics steps; the
    /// game may run slower than real time, the video never does. Default.
    #[default]
    Recording,
    /// No recording.
    Off,
}

impl RecordingMode {
    const ALL: [RecordingMode; 3] = [
        RecordingMode::Gameplay,
        RecordingMode::Recording,
        RecordingMode::Off,
    ];

    /// The next (`step` = 1) or previous (`step` = -1) mode, wrapping around.
    pub fn cycle(self, step: i32) -> RecordingMode {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap_or(0) as i32;
        Self::ALL[(i + step).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Language code (`en`, `sv`).
    pub language: String,
    /// Sound volume in percent, 0–100.
    pub volume: u8,
    pub recording_mode: RecordingMode,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: Lang::En.code().to_owned(),
            volume: 80,
            recording_mode: RecordingMode::default(),
        }
    }
}

/// Fields of older `settings.json` versions.
#[derive(Deserialize)]
struct Legacy {
    /// Before `recording_mode`: recording on/off.
    recording: Option<bool>,
    /// Present in newer files, which then ignore `recording`.
    recording_mode: Option<serde_json::Value>,
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

/// The program's data folder (overridable with `<ENV_PREFIX>_DATA_DIR`).
pub fn data_dir() -> PathBuf {
    let env = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty());
    if let Some(dir) = config::env("DATA_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(windows) {
        if let Some(appdata) = env("APPDATA") {
            return PathBuf::from(appdata).join(APP_NAME);
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = env("HOME") {
            return PathBuf::from(home)
                .join("Library/Application Support")
                .join(APP_NAME);
        }
    } else if let Some(data) = env("XDG_DATA_HOME") {
        return PathBuf::from(data).join(APP_ID);
    } else if let Some(home) = env("HOME") {
        return PathBuf::from(home).join(".local/share").join(APP_ID);
    }
    PathBuf::from(format!("{APP_ID}-data"))
}

pub fn default_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// `<ENV_PREFIX>_LANG`, which overrides the saved language for this run.
pub fn env_lang() -> Option<Lang> {
    config::env_str("LANG").and_then(|code| Lang::from_code(&code))
}

/// The saved settings. A missing or broken file gives the defaults, and
/// missing or invalid fields get their default values.
pub fn load(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Settings from the text of `settings.json`. Older files with
/// `"recording": true/false` become Prioritize gameplay / Off.
fn parse(text: &str) -> Settings {
    let Ok(mut s) = serde_json::from_str::<Settings>(text) else {
        return Settings::default();
    };
    if let Ok(Legacy {
        recording: Some(on),
        recording_mode: None,
    }) = serde_json::from_str::<Legacy>(text)
    {
        s.recording_mode = if on {
            RecordingMode::Recording
        } else {
            RecordingMode::Off
        };
    }
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
        assert_eq!(defaults.recording_mode, RecordingMode::Recording, "default");
        assert_eq!(defaults.lang(), Lang::En, "English by default");

        let mut s = Settings {
            language: "sv".into(),
            ..Settings::default()
        };
        s.change_volume(-3);
        s.recording_mode = RecordingMode::Recording;
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"volume\": 50"), "{text}");
        assert!(text.contains("\"recording_mode\": \"recording\""), "{text}");
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
        let mode = s.recording_mode;
        assert_eq!(
            (s.volume, mode, s.lang()),
            (30, RecordingMode::Recording, Lang::En)
        );
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
    fn old_recording_flags_are_migrated() {
        let mode = |text: &str| parse(text).recording_mode;
        assert_eq!(mode(r#"{ "recording": true }"#), RecordingMode::Recording);
        assert_eq!(mode(r#"{ "recording": false }"#), RecordingMode::Off);
        assert_eq!(
            mode(r#"{ "volume": 40 }"#),
            RecordingMode::Recording,
            "default"
        );
        // A saved choice is kept, also when it is not the default.
        for saved in ["gameplay", "recording", "off"] {
            let text = format!(r#"{{ "recording_mode": "{saved}" }}"#);
            let s = parse(&text);
            assert!(
                serde_json::to_string(&s).unwrap().contains(saved),
                "{saved}"
            );
        }
        let kept = r#"{ "recording": true, "recording_mode": "gameplay" }"#;
        assert_eq!(mode(kept), RecordingMode::Gameplay);
        // The new field wins over the old one.
        let both = r#"{ "recording": false, "recording_mode": "recording" }"#;
        assert_eq!(mode(both), RecordingMode::Recording);
        let full = parse(r#"{ "language": "sv", "volume": 30, "recording": false }"#);
        assert_eq!(
            (full.lang(), full.volume),
            (Lang::Sv, 30),
            "other fields kept"
        );
        // Saving writes only the new field.
        let text = serde_json::to_string(&full).unwrap();
        assert!(text.contains("\"recording_mode\":\"off\"") && !text.contains("\"recording\":"));
        // An unknown mode is ignored like any broken value.
        assert_eq!(
            mode(r#"{ "recording_mode": "sometimes" }"#),
            RecordingMode::default()
        );
    }

    #[test]
    fn recording_modes_cycle_both_ways() {
        let m = RecordingMode::Gameplay;
        assert_eq!(m.cycle(1), RecordingMode::Recording);
        assert_eq!(m.cycle(1).cycle(1), RecordingMode::Off);
        assert_eq!(m.cycle(-1), RecordingMode::Off);
        assert_eq!(m.cycle(3), m);
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
