//! Translations. English is the default; Swedish can be chosen in the settings.
//!
//! Every text shown to the player lives in `CATALOG` as (key, English,
//! Swedish). Code refers to texts by key with the `t!` macro, which fills
//! in named placeholders: `t!("results.score", score = 120)`.
//!
//! The language is kept per thread (the game runs on one thread); a worker
//! thread that produces texts copies the language when it is started.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    Sv,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Sv];

    /// Code used in the settings file and `RUSTYHUSKY_LANG`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Sv => "sv",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        let code = code.trim().to_lowercase();
        Lang::ALL
            .into_iter()
            .find(|l| code == l.code() || code.starts_with(&format!("{}_", l.code())))
    }

    /// The next language, for the language setting.
    pub fn next(self) -> Lang {
        match self {
            Lang::En => Lang::Sv,
            Lang::Sv => Lang::En,
        }
    }
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
}

pub fn lang() -> Lang {
    CURRENT.with(Cell::get)
}

pub fn set_lang(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

/// The text for `key` in the current language.
pub fn tr(key: &str) -> &'static str {
    tr_in(lang(), key)
}

/// The text for `key` in `lang`. An unknown key is a programming error; in
/// release builds the key itself is shown rather than crashing the game.
pub fn tr_in(lang: Lang, key: &str) -> &'static str {
    static INDEX: OnceLock<HashMap<&'static str, (&'static str, &'static str)>> = OnceLock::new();
    let index = INDEX.get_or_init(|| CATALOG.iter().map(|&(k, en, sv)| (k, (en, sv))).collect());
    match index.get(key) {
        Some(&(en, sv)) => match lang {
            Lang::En => en,
            Lang::Sv => sv,
        },
        None => {
            debug_assert!(false, "missing translation key: {key}");
            CATALOG[0].1
        }
    }
}

/// Fills `{name}` placeholders in a translated text.
pub fn fill(text: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = text.to_owned();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// `t!("key")` gives the `&'static str` text; `t!("key", name = value, …)`
/// gives a `String` with the placeholders filled in.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::tr($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::tr($key),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

/// (key, English, Swedish)
const CATALOG: &[(&str, &str, &str)] = &[
    ("missing", "?", "?"),
    // Main menu
    ("app.title", "RUSTYHUSKY", "RUSTYHUSKY"),
    ("menu.subtitle", "A starter template for games in Rust", "En startmall för spel i Rust"),
    ("menu.play", "Play", "Spela"),
    ("menu.settings", "Settings", "Inställningar"),
    ("menu.quit", "Quit", "Avsluta"),
    ("menu.help", "Arrow keys and Enter, or the mouse", "Piltangenter och Enter, eller musen"),
    // Settings
    ("settings.title", "SETTINGS", "INSTÄLLNINGAR"),
    ("settings.language", "Language: English", "Språk: Svenska"),
    ("settings.volume", "<  Volume: {percent}%  >", "<  Volym: {percent} %  >"),
    ("settings.recording_on", "Recording: On", "Inspelning: På"),
    ("settings.recording_off", "Recording: Off", "Inspelning: Av"),
    ("settings.back", "Back", "Tillbaka"),
    ("settings.help", "Left/Right: change   Esc: back", "Vänster/höger: ändra   Esc: tillbaka"),
    ("settings.recording_note", "When recording is on, every round is recorded automatically. You choose afterwards whether to save the video.", "När inspelning är på spelas varje omgång in automatiskt. Efteråt väljer du om videon ska sparas."),
    ("settings.save_failed", "Could not save the settings: {error}", "Kunde inte spara inställningarna: {error}"),
    // Pause
    ("pause.title", "PAUSED", "PAUS"),
    ("pause.resume", "Resume", "Fortsätt"),
    ("pause.abort", "Abort round", "Avbryt omgången"),
    ("pause.help", "Esc or P: resume", "Esc eller P: fortsätt"),
    ("hud.rec", "REC", "REC"),
    // Results
    ("results.complete", "LEVEL {level} COMPLETE", "NIVÅ {level} KLAR"),
    ("results.aborted", "ROUND ABORTED", "OMGÅNGEN AVBRUTEN"),
    ("results.score", "Score: {score}", "Poäng: {score}"),
    ("results.time", "Time: {time} s", "Tid: {time} s"),
    ("results.next", "Next round", "Nästa omgång"),
    ("results.menu", "Main menu", "Huvudmeny"),
    ("results.leave_note", "An unsaved video is discarded when you leave this screen.", "En osparad video kastas när du lämnar den här vyn."),
    // Video
    ("video.save", "Save video", "Spara video"),
    ("video.discard", "Discard video", "Kasta video"),
    ("video.status_off", "Recording is off (see Settings).", "Inspelning är avstängd (se Inställningar)."),
    ("video.status_recording", "Recording...", "Spelar in..."),
    ("video.status_finishing", "Finishing the video...", "Färdigställer videon..."),
    ("video.status_ready", "The video is ready but not saved yet.", "Videon är klar men inte sparad ännu."),
    ("video.status_saving", "Saving the video...", "Sparar videon..."),
    ("video.status_saved", "The video was saved as {path}", "Videon sparades som {path}"),
    ("video.status_discarded", "The video was discarded.", "Videon kastades."),
    ("video.no_ffmpeg", "FFmpeg was not found, so rounds cannot be recorded. Install FFmpeg to record videos.", "FFmpeg hittades inte, så omgångar kan inte spelas in. Installera FFmpeg för att spela in video."),
    ("video.no_x264", "FFmpeg has no libx264 encoder, so rounds cannot be recorded.", "FFmpeg saknar kodaren libx264, så omgångar kan inte spelas in."),
    ("video.start_failed", "Recording could not start: {error}", "Inspelningen kunde inte starta: {error}"),
    ("video.record_failed", "Recording failed: {error}", "Inspelningen misslyckades: {error}"),
    ("video.save_failed", "Could not save the video: {error}", "Kunde inte spara videon: {error}"),
    ("video.nothing", "there is no recording to save", "det finns ingen inspelning att spara"),
    ("video.crashed", "the video thread crashed", "videotråden kraschade"),
    ("video.ffmpeg_start", "could not start ffmpeg: {error}", "kunde inte starta ffmpeg: {error}"),
    ("video.ffmpeg_failed", "ffmpeg failed: {error}", "ffmpeg misslyckades: {error}"),
    ("video.ffmpeg_timeout", "ffmpeg did not finish in time and was stopped", "ffmpeg blev inte klar i tid och stoppades"),
    ("video.pipe", "could not send frames to ffmpeg: {error}", "kunde inte skicka bilder till ffmpeg: {error}"),
    ("video.empty", "the recording is empty", "inspelningen är tom"),
    ("video.mkdir", "could not create the folder {path}: {error}", "kunde inte skapa mappen {path}: {error}"),
    ("video.write_sound", "could not write the sound: {error}", "kunde inte skriva ljudet: {error}"),
    ("video.write_file", "could not save {path}: {error}", "kunde inte spara {path}: {error}"),
    // The example game
    ("example.title", "Your game here", "Ditt spel här"),
    ("example.level", "Level {level}", "Nivå {level}"),
    ("example.score", "Score {score}", "Poäng {score}"),
    ("example.finish", "Finish level", "Avsluta nivån"),
    ("example.help", "Space: +1 point   Enter: finish level   Esc: pause", "Mellanslag: +1 poäng   Enter: avsluta nivån   Esc: paus"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(text: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let Some(len) = rest[start..].find('}') else {
                break;
            };
            let name = &rest[start + 1..start + len];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                out.insert(name.to_owned());
            }
            rest = &rest[start + len..];
        }
        out
    }

    #[test]
    fn catalog_is_complete_and_consistent() {
        let mut keys = BTreeSet::new();
        for &(key, en, sv) in CATALOG {
            assert!(keys.insert(key), "duplicate key {key}");
            assert!(!en.is_empty() && !sv.is_empty(), "{key} is empty");
            assert_eq!(
                placeholders(en),
                placeholders(sv),
                "{key}: placeholders differ"
            );
        }
    }

    /// Every `t!("…")` in the source must exist in the catalog, so a typo
    /// is caught here rather than shown to the player.
    #[test]
    fn every_key_used_in_the_code_exists() {
        // Every Rust file under src/, read at test time so new files are
        // checked without anyone having to list them here.
        fn rust_files(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    rust_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("i18n.rs")
                {
                    out.push(std::fs::read_to_string(&path).unwrap());
                }
            }
        }
        let mut sources = Vec::new();
        rust_files(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut sources,
        );
        assert!(
            sources.len() >= 10,
            "found only {} source files",
            sources.len()
        );
        let known: BTreeSet<&str> = CATALOG.iter().map(|&(k, _, _)| k).collect();
        let mut used = 0;
        for src in &sources {
            let mut rest = src.as_str();
            while let Some(at) = rest.find("t!(\"") {
                // Skip `format!(` and other macros that merely end in `t!(`.
                let standalone = rest[..at]
                    .chars()
                    .last()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_');
                rest = &rest[at + 4..];
                if standalone {
                    let key = &rest[..rest.find('"').unwrap()];
                    assert!(known.contains(key), "unknown translation key: {key}");
                    used += 1;
                }
            }
        }
        assert!(used > 40, "found only {used} uses");
    }

    #[test]
    fn placeholders_are_filled_in_both_languages() {
        let args: &[(&str, &dyn Display)] = &[("level", &2)];
        assert_eq!(
            fill(tr_in(Lang::En, "results.complete"), args),
            "LEVEL 2 COMPLETE"
        );
        assert_eq!(
            fill(tr_in(Lang::Sv, "results.complete"), args),
            "NIVÅ 2 KLAR"
        );
    }

    #[test]
    fn language_is_per_thread_and_defaults_to_english() {
        assert_eq!(lang(), Lang::En);
        assert_eq!(t!("menu.play"), "Play");
        std::thread::spawn(|| {
            set_lang(Lang::Sv);
            assert_eq!(t!("menu.play"), "Spela");
            assert_eq!(t!("results.score", score = 7), "Poäng: 7");
        })
        .join()
        .unwrap();
        assert_eq!(t!("menu.play"), "Play", "other threads are not affected");
        assert_eq!(Lang::from_code("sv_SE.UTF-8"), Some(Lang::Sv));
        assert_eq!(Lang::from_code("EN"), Some(Lang::En));
        assert_eq!(Lang::from_code("de"), None);
        assert_eq!(Lang::En.next(), Lang::Sv);
    }
}
