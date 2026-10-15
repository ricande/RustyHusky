//! Screens and the flow between them: main menu → settings, or a round →
//! (pause) → results → next round or main menu.
//!
//! Driven by abstract `Input` and window-free, so the whole flow is tested
//! headlessly. The game itself is any type implementing `Game`. Recording
//! is done by `capture.rs`, which reports its progress in `App::video` and
//! takes the player's choices from `App::video_request`.

use std::path::PathBuf;

use macroquad::math::Rect;

use crate::audio::Sfx;
use crate::game::{Game, RoundResult, FIXED_DT};
use crate::settings::{self, RecordingMode, Settings};
use crate::t;
use crate::ui::{button_column, button_column_w, button_row, choose, Button, Input, Key};

pub enum Screen<G> {
    Menu,
    Settings,
    Playing {
        game: G,
        paused: bool,
    },
    /// After a round; `result` is `None` if it was aborted.
    Results {
        level: u32,
        result: Option<RoundResult>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Play,
    Settings,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    Language,
    Volume,
    Recording,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseAction {
    Resume,
    Abort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultsAction {
    SaveVideo,
    DiscardVideo,
    NextRound,
    Menu,
}

/// The current round's video, as shown on the results screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoStatus {
    /// Recording is switched off in the settings.
    Off,
    /// Recording is on but cannot work (e.g. ffmpeg is missing).
    Unavailable(String),
    Recording,
    /// The round is over and the encoder is finishing the temporary file.
    Finishing,
    /// A temporary video waits for Save or Discard; `error` is a failed
    /// save, which can be retried.
    Ready {
        error: Option<String>,
    },
    Saving,
    Saved(PathBuf),
    Discarded,
    Failed(String),
}

impl VideoStatus {
    /// Work is going on; the player waits before leaving the results.
    pub fn busy(&self) -> bool {
        matches!(
            self,
            VideoStatus::Recording | VideoStatus::Finishing | VideoStatus::Saving
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoRequest {
    Save,
    Discard,
}

pub fn menu_buttons() -> Vec<Button<MenuAction>> {
    button_column(
        500.0,
        vec![
            (t!("menu.play").into(), MenuAction::Play),
            (t!("menu.settings").into(), MenuAction::Settings),
            (t!("menu.quit").into(), MenuAction::Quit),
        ],
    )
}

pub fn settings_buttons(s: &Settings) -> Vec<Button<SettingsAction>> {
    let recording = match s.recording_mode {
        RecordingMode::Gameplay => t!("settings.mode_gameplay"),
        RecordingMode::Recording => t!("settings.mode_recording"),
        RecordingMode::Off => t!("settings.mode_off"),
    };
    let mut buttons = button_column_w(
        240.0,
        SETTINGS_W,
        vec![
            (t!("settings.language").into(), SettingsAction::Language),
            (
                t!("settings.volume", percent = s.volume),
                SettingsAction::Volume,
            ),
            (recording.into(), SettingsAction::Recording),
        ],
    );
    buttons.extend(button_column(
        760.0,
        vec![(t!("settings.back").into(), SettingsAction::Back)],
    ));
    buttons
}

/// Width of the settings buttons, wide enough for the recording modes.
const SETTINGS_W: f32 = 460.0;

pub fn pause_buttons() -> Vec<Button<PauseAction>> {
    button_column(
        460.0,
        vec![
            (t!("pause.resume").into(), PauseAction::Resume),
            (t!("pause.abort").into(), PauseAction::Abort),
        ],
    )
}

/// Save/Discard while a video is ready; "Next round" after a completed
/// level; "Main menu". Leaving waits while the video is busy.
pub fn results_buttons(completed: bool, video: &VideoStatus) -> Vec<Button<ResultsAction>> {
    let mut buttons = Vec::new();
    if matches!(video, VideoStatus::Ready { .. }) {
        buttons = button_row(
            Rect::new(40.0, 560.0, 460.0, 52.0),
            vec![
                (t!("video.save").into(), ResultsAction::SaveVideo),
                (t!("video.discard").into(), ResultsAction::DiscardVideo),
            ],
        );
    }
    let mut leave = Vec::new();
    if completed {
        leave.push((t!("results.next").into(), ResultsAction::NextRound));
    }
    leave.push((t!("results.menu").into(), ResultsAction::Menu));
    for mut b in button_column(700.0, leave) {
        b.enabled = !video.busy();
        buttons.push(b);
    }
    buttons
}

pub struct App<G> {
    pub screen: Screen<G>,
    /// Keyboard focus: an index into the current screen's buttons.
    pub focus: usize,
    pub settings: Settings,
    /// Where the settings are saved (`None`: not saved, e.g. in tests).
    pub settings_path: Option<PathBuf>,
    /// Problem saving the settings, shown on the settings screen.
    pub settings_error: Option<String>,
    /// Why rounds can never be recorded (e.g. ffmpeg is missing).
    pub recording_unavailable: Option<String>,
    /// The level of the current or last round (1 = first).
    pub level: u32,
    /// Increases whenever a round starts; `capture.rs` starts a recording
    /// for each new value.
    pub round: u64,
    /// Played (unpaused) time of the current round, in seconds.
    pub play_time: f32,
    pub video: VideoStatus,
    /// The player's Save/Discard choice, taken by `capture.rs`.
    pub video_request: Option<VideoRequest>,
    /// The player chose Quit.
    pub quit: bool,
    /// A round started this frame (see `take_round_started`).
    round_started: bool,
    /// Game input not yet delivered to the game; it gets it at its next
    /// simulation step (`advance`), so no press is lost or repeated.
    game_input: Input,
    /// Sounds of the menus (never recorded).
    ui_sounds: Vec<Sfx>,
}

impl<G: Game> App<G> {
    pub fn new(settings: Settings, settings_path: Option<PathBuf>) -> Self {
        App {
            screen: Screen::Menu,
            focus: 0,
            settings,
            settings_path,
            settings_error: None,
            recording_unavailable: None,
            level: 1,
            round: 0,
            play_time: 0.0,
            video: VideoStatus::Off,
            video_request: None,
            quit: false,
            round_started: false,
            game_input: Input::default(),
            ui_sounds: Vec::new(),
        }
    }

    fn go(&mut self, screen: Screen<G>) {
        self.screen = screen;
        self.focus = 0;
    }

    fn start_round(&mut self, level: u32) {
        self.level = level;
        self.go(Screen::Playing {
            game: G::new(level),
            paused: false,
        });
        self.round += 1;
        self.round_started = true;
        self.game_input = Input::default();
        self.play_time = 0.0;
        self.video = VideoStatus::Off;
        self.video_request = None;
    }

    /// The game, if it is running (not paused): what is drawn into the video.
    pub fn running_game(&self) -> Option<&G> {
        match &self.screen {
            Screen::Playing {
                game,
                paused: false,
            } => Some(game),
            _ => None,
        }
    }

    pub fn paused(&self) -> bool {
        matches!(self.screen, Screen::Playing { paused: true, .. })
    }

    /// A round is being played (paused or not).
    pub fn in_round(&self) -> bool {
        matches!(self.screen, Screen::Playing { .. })
    }

    /// True once after a round has started. Its first video frame shows the
    /// state before any simulation step, so the caller does not simulate
    /// in that frame.
    pub fn take_round_started(&mut self) -> bool {
        std::mem::take(&mut self.round_started)
    }

    /// Handles one frame of input.
    pub fn handle(&mut self, input: &Input) {
        match &mut self.screen {
            Screen::Menu => self.handle_menu(input),
            Screen::Settings => self.handle_settings(input),
            Screen::Playing { game, paused } => {
                if *paused {
                    let resume = input.key(Key::Escape) || input.key(Key::P);
                    let chosen = choose(&pause_buttons(), &mut self.focus, input);
                    if resume || chosen == Some(PauseAction::Resume) {
                        *paused = false;
                    } else if chosen == Some(PauseAction::Abort) {
                        self.ui_sounds.push(Sfx::Click);
                        let level = self.level;
                        self.go(Screen::Results {
                            level,
                            result: None,
                        });
                    }
                } else if input.key(Key::Escape) || input.key(Key::P) {
                    *paused = true;
                    self.focus = 0;
                } else if game.finished().is_none() {
                    self.game_input.merge(input);
                }
            }
            Screen::Results { level, result } => {
                let (level, completed) = (*level, result.is_some());
                self.handle_results(input, level, completed);
            }
        }
    }

    fn handle_menu(&mut self, input: &Input) {
        match choose(&menu_buttons(), &mut self.focus, input) {
            Some(MenuAction::Play) => {
                self.ui_sounds.push(Sfx::Click);
                self.start_round(1);
            }
            Some(MenuAction::Settings) => {
                self.ui_sounds.push(Sfx::Click);
                self.go(Screen::Settings);
            }
            Some(MenuAction::Quit) => self.quit = true,
            None => {}
        }
    }

    fn handle_settings(&mut self, input: &Input) {
        let buttons = settings_buttons(&self.settings);
        let step = i32::from(input.key(Key::Right)) - i32::from(input.key(Key::Left));
        let chosen = choose(&buttons, &mut self.focus, input);
        let focused = buttons[self.focus.min(buttons.len() - 1)].action;
        let changed = match (chosen, focused) {
            (Some(SettingsAction::Back), _) => {
                self.go(Screen::Menu);
                false
            }
            (Some(SettingsAction::Volume), _) => {
                // A click on the left half lowers, on the right half raises;
                // Enter raises and wraps around to 0.
                let rect = buttons[self.focus].rect;
                match input.pointer.filter(|_| input.left_pressed) {
                    Some(p) if p.x < rect.center().x => self.settings.change_volume(-1),
                    Some(_) => self.settings.change_volume(1),
                    None if self.settings.volume == 100 => self.settings.volume = 0,
                    None => self.settings.change_volume(1),
                }
                true
            }
            (Some(action), _) => self.change_setting(action, 1),
            (None, action) if step != 0 => self.change_setting(action, step),
            _ => false,
        };
        if input.key(Key::Escape) {
            self.go(Screen::Menu);
        }
        if changed {
            self.ui_sounds.push(Sfx::Click);
            self.save_settings();
        }
    }

    /// Changes a setting by `step` (−1 or +1); returns whether it changed.
    fn change_setting(&mut self, action: SettingsAction, step: i32) -> bool {
        match action {
            SettingsAction::Language => {
                let lang = crate::i18n::lang().next();
                crate::i18n::set_lang(lang);
                self.settings.language = lang.code().to_owned();
            }
            SettingsAction::Volume => {
                let before = self.settings.volume;
                self.settings.change_volume(step);
                return self.settings.volume != before;
            }
            SettingsAction::Recording => {
                self.settings.recording_mode = self.settings.recording_mode.cycle(step)
            }
            SettingsAction::Back => return false,
        }
        true
    }

    fn save_settings(&mut self) {
        self.settings_error = self
            .settings_path
            .as_ref()
            .and_then(|path| settings::save(path, &self.settings).err());
    }

    fn handle_results(&mut self, input: &Input, level: u32, completed: bool) {
        let buttons = results_buttons(completed, &self.video);
        let mut chosen = choose(&buttons, &mut self.focus, input);
        if input.key(Key::Escape) && !self.video.busy() {
            chosen = Some(ResultsAction::Menu);
        }
        let Some(action) = chosen else { return };
        self.ui_sounds.push(Sfx::Click);
        match action {
            ResultsAction::SaveVideo => {
                self.video_request = Some(VideoRequest::Save);
                self.video = VideoStatus::Saving;
            }
            ResultsAction::DiscardVideo => {
                self.video_request = Some(VideoRequest::Discard);
                self.video = VideoStatus::Discarded;
            }
            ResultsAction::NextRound | ResultsAction::Menu => {
                // An unsaved video is thrown away when the player leaves.
                if matches!(self.video, VideoStatus::Ready { .. }) {
                    self.video_request = Some(VideoRequest::Discard);
                }
                if action == ResultsAction::NextRound {
                    self.start_round(level + 1);
                } else {
                    self.go(Screen::Menu);
                }
            }
        }
    }

    /// One simulation step of `steps` × `FIXED_DT` for the running game:
    /// the input gathered since the last step is handed to the game once,
    /// then it is updated `steps` times (fewer if it finishes). Does nothing
    /// for `steps` = 0, so input waits for the next real step.
    pub fn advance(&mut self, steps: u32) {
        let Screen::Playing {
            game,
            paused: false,
        } = &mut self.screen
        else {
            return;
        };
        if steps == 0 || game.finished().is_some() {
            return;
        }
        game.handle(&self.game_input.take());
        for _ in 0..steps {
            if game.finished().is_some() {
                break;
            }
            game.update(FIXED_DT);
            self.play_time += FIXED_DT;
        }
    }

    /// The menus' sounds since the last call. They are played but never
    /// recorded.
    pub fn take_ui_sounds(&mut self) -> Vec<Sfx> {
        std::mem::take(&mut self.ui_sounds)
    }

    /// The game's sounds since the last call, for playing and recording.
    pub fn take_game_sounds(&mut self) -> Vec<Sfx> {
        match &mut self.screen {
            Screen::Playing { game, .. } => game.take_sounds(),
            _ => Vec::new(),
        }
    }

    /// Shows the results once the game reports a finished level. Called
    /// after the frame has been recorded, so the video ends on the game's
    /// last picture and never shows the results screen.
    pub fn end_round_if_finished(&mut self) {
        if let Screen::Playing { game, .. } = &self.screen {
            if let Some(result) = game.finished() {
                self.ui_sounds.push(Sfx::Success);
                let level = self.level;
                self.go(Screen::Results {
                    level,
                    result: Some(result),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::example::{ExampleGame, FINISH_BUTTON};
    use crate::game::FIXED_DT;
    use macroquad::math::Vec2;

    type TestApp = App<ExampleGame>;

    fn app() -> TestApp {
        App::new(Settings::default(), None)
    }

    fn key(k: Key) -> Input {
        Input {
            keys: vec![k],
            held: vec![k],
            ..Default::default()
        }
    }

    fn click(p: Vec2) -> Input {
        Input {
            pointer: Some(p),
            left_pressed: true,
            left_down: true,
            ..Default::default()
        }
    }

    fn button<A: PartialEq>(buttons: Vec<Button<A>>, action: A) -> Vec2 {
        buttons
            .into_iter()
            .find(|b| b.action == action)
            .expect("button exists")
            .rect
            .center()
    }

    /// One frame as `main` runs it, without drawing or recording.
    /// One 60 Hz frame as `main` runs it in real time, without drawing or
    /// recording: a new round's first frame does not simulate.
    fn frame(app: &mut TestApp, input: &Input) {
        app.handle(input);
        if !app.take_round_started() {
            app.advance(2);
        }
        app.end_round_if_finished();
    }

    fn results(app: &TestApp) -> Option<(u32, Option<RoundResult>)> {
        match &app.screen {
            Screen::Results { level, result } => Some((*level, *result)),
            _ => None,
        }
    }

    #[test]
    fn menu_round_results_next_round_and_back_to_menu() {
        let mut app = app();
        frame(&mut app, &key(Key::Enter)); // Play
        assert!(app.running_game().is_some());
        assert_eq!((app.round, app.level), (1, 1));
        frame(&mut app, &key(Key::Space)); // the example scores a point
        frame(&mut app, &Input::default());
        assert!((app.play_time - 4.0 * FIXED_DT).abs() < 1e-6);
        frame(&mut app, &key(Key::Enter)); // finishes the level
        assert_eq!(results(&app), Some((1, Some(RoundResult { score: 1 }))));
        assert!(app.take_ui_sounds().contains(&Sfx::Success));

        // Next round (the focus starts on it, recording is off).
        frame(&mut app, &key(Key::Enter));
        assert!(app.running_game().is_some());
        assert_eq!((app.round, app.level), (2, 2));
        assert_eq!(app.play_time, 0.0, "time restarts at the first frame");
        frame(&mut app, &click(FINISH_BUTTON.center()));
        assert_eq!(results(&app), Some((2, Some(RoundResult { score: 0 }))));

        let menu = button(results_buttons(true, &app.video), ResultsAction::Menu);
        frame(&mut app, &click(menu));
        assert!(matches!(app.screen, Screen::Menu));
        frame(&mut app, &click(button(menu_buttons(), MenuAction::Play)));
        assert_eq!(
            (app.round, app.level),
            (3, 1),
            "Play starts again at level 1"
        );
    }

    #[test]
    fn pause_freezes_the_game_and_can_abort() {
        let mut app = app();
        frame(&mut app, &key(Key::Enter));
        frame(&mut app, &key(Key::Escape));
        assert!(app.paused());
        assert!(
            app.running_game().is_none(),
            "nothing to record while paused"
        );
        let before = app.play_time;
        frame(&mut app, &key(Key::Down)); // goes to the pause menu, not the game
        frame(&mut app, &Input::default());
        assert_eq!(app.play_time, before, "time stands still");
        assert!(app.paused());
        frame(&mut app, &key(Key::P));
        assert!(!app.paused(), "P resumes");
        frame(&mut app, &key(Key::P));
        assert!(app.paused(), "P pauses too");
        frame(&mut app, &key(Key::Escape));
        assert!(!app.paused(), "Esc resumes");
        assert!(app.play_time > before);

        frame(&mut app, &key(Key::Escape));
        frame(
            &mut app,
            &click(button(pause_buttons(), PauseAction::Abort)),
        );
        assert_eq!(results(&app), Some((1, None)));
        let b = results_buttons(false, &app.video);
        assert!(
            b.iter().all(|b| b.action != ResultsAction::NextRound),
            "no next round after an abort"
        );
        frame(&mut app, &key(Key::Escape));
        assert!(matches!(app.screen, Screen::Menu));
    }

    #[test]
    fn results_offer_save_and_discard_and_wait_while_busy() {
        let mut app = app();
        frame(&mut app, &key(Key::Enter));
        app.video = VideoStatus::Recording; // as `capture` sets it
        frame(&mut app, &key(Key::Enter));
        assert!(results(&app).is_some());
        app.video = VideoStatus::Finishing;
        let b = results_buttons(true, &app.video);
        assert!(
            b.iter().all(|b| !b.enabled),
            "wait while the video is finished"
        );
        frame(&mut app, &key(Key::Enter));
        frame(&mut app, &key(Key::Escape));
        assert!(results(&app).is_some(), "cannot leave while busy");

        app.video = VideoStatus::Ready { error: None };
        let at = button(results_buttons(true, &app.video), ResultsAction::SaveVideo);
        frame(&mut app, &click(at));
        assert_eq!(app.video_request, Some(VideoRequest::Save));
        assert_eq!(app.video, VideoStatus::Saving);
        app.video_request = None;
        app.video = VideoStatus::Ready { error: None };
        let at = button(
            results_buttons(true, &app.video),
            ResultsAction::DiscardVideo,
        );
        frame(&mut app, &click(at));
        assert_eq!(app.video_request, Some(VideoRequest::Discard));
        assert_eq!(app.video, VideoStatus::Discarded);
        assert!(results(&app).is_some(), "still on the results");

        // Leaving with an unsaved video discards it.
        app.video_request = None;
        app.video = VideoStatus::Ready { error: None };
        frame(&mut app, &key(Key::Escape));
        assert!(matches!(app.screen, Screen::Menu));
        assert_eq!(app.video_request, Some(VideoRequest::Discard));
    }

    #[test]
    fn settings_change_and_are_saved() {
        let dir = crate::tests::temp_dir("app-settings");
        let path = dir.join("settings.json");
        std::thread::spawn(move || {
            let mut app: TestApp = App::new(Settings::default(), Some(path.clone()));
            frame(
                &mut app,
                &click(button(menu_buttons(), MenuAction::Settings)),
            );
            assert!(matches!(app.screen, Screen::Settings));

            frame(&mut app, &key(Key::Enter)); // language
            assert_eq!(crate::i18n::lang(), crate::i18n::Lang::Sv);
            assert_eq!(app.settings.language, "sv");

            frame(&mut app, &key(Key::Down)); // volume
            frame(&mut app, &key(Key::Left));
            assert_eq!(app.settings.volume, 70);
            let volume = settings_buttons(&app.settings)[1].rect;
            frame(
                &mut app,
                &click(Vec2::new(volume.right() - 5.0, volume.center().y)),
            );
            frame(
                &mut app,
                &click(Vec2::new(volume.right() - 5.0, volume.center().y)),
            );
            assert_eq!(app.settings.volume, 90);
            frame(
                &mut app,
                &click(Vec2::new(volume.x + 5.0, volume.center().y)),
            );
            assert_eq!(app.settings.volume, 80);

            frame(&mut app, &key(Key::Down)); // recording
            assert_eq!(
                app.settings.recording_mode,
                RecordingMode::Recording,
                "default"
            );
            frame(&mut app, &key(Key::Enter));
            assert_eq!(app.settings.recording_mode, RecordingMode::Off);
            frame(&mut app, &key(Key::Left));
            frame(&mut app, &key(Key::Left));
            assert_eq!(app.settings.recording_mode, RecordingMode::Gameplay);
            assert_eq!(app.settings_error, None);
            assert_eq!(settings::load(&path), app.settings, "saved on every change");

            frame(&mut app, &key(Key::Escape));
            assert!(matches!(app.screen, Screen::Menu));
        })
        .join()
        .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_settings_save_is_shown_not_fatal() {
        let dir = crate::tests::temp_dir("app-settings-fail");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("file"), "x").unwrap();
        let mut app: TestApp = App::new(Settings::default(), Some(dir.join("file/settings.json")));
        app.screen = Screen::Settings;
        app.focus = 2;
        frame(&mut app, &key(Key::Enter));
        assert_eq!(
            app.settings.recording_mode,
            RecordingMode::Off,
            "the change still applies"
        );
        assert!(app.settings_error.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A press in a frame without a simulation step (or while a video
    /// frame waits) reaches the game once, at its next step.
    #[test]
    fn presses_wait_for_the_next_step_and_count_once() {
        let mut app = app();
        frame(&mut app, &key(Key::Enter)); // Play
        app.handle(&key(Key::Space));
        app.advance(0);
        app.handle(&Input::default());
        app.advance(0);
        assert_eq!(app.play_time, 0.0, "no step yet");
        app.advance(2);
        app.advance(2);
        app.handle(&key(Key::Enter)); // finish: the score is now final
        app.advance(2);
        app.end_round_if_finished();
        assert_eq!(results(&app), Some((1, Some(RoundResult { score: 1 }))));
    }

    #[test]
    fn menu_sounds_are_kept_apart_from_game_sounds() {
        let mut app = app();
        frame(&mut app, &key(Key::Enter)); // Play: a click
        frame(&mut app, &key(Key::Space)); // the game's blip
        assert_eq!(app.take_ui_sounds(), vec![Sfx::Click]);
        assert_eq!(app.take_game_sounds(), vec![Sfx::Blip]);
    }

    #[test]
    fn quit_and_clicks_outside_buttons() {
        let mut app = app();
        frame(&mut app, &click(Vec2::new(5.0, 5.0)));
        assert!(matches!(app.screen, Screen::Menu));
        // A click in the letterbox bars has no logical position at all.
        frame(
            &mut app,
            &Input {
                left_pressed: true,
                ..Default::default()
            },
        );
        assert!(matches!(app.screen, Screen::Menu));
        frame(&mut app, &click(button(menu_buttons(), MenuAction::Quit)));
        assert!(app.quit);
    }
}
