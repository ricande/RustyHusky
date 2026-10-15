//! The example game: a placeholder that shows "Your game here" and lets the
//! player finish the level, so the whole flow (menu → round → results →
//! next round) can be tried. Replace this module with your own game; see
//! `game.rs` for the interface and `docs/GAME_RULES.md` for its behaviour.

use macroquad::color::WHITE;
use macroquad::math::{vec2, Rect};

use crate::audio::Sfx;
use crate::game::{Game, RoundResult};
use crate::i18n::Text;
use crate::t;
use crate::ui::{Input, Key};
use crate::view::{draw_button, with_alpha, View, ACCENT, FIELD_W, NEON, TEXT};

/// The "Finish level" button.
pub const FINISH_BUTTON: Rect = Rect {
    x: 120.0,
    y: 700.0,
    w: 300.0,
    h: 56.0,
};

/// Centre and radius of the moving dot, which shows that time runs (and
/// that the recording is smooth).
const ORBIT_CENTRE: (f32, f32) = (FIELD_W / 2.0, 470.0);
const ORBIT_R: f32 = 120.0;

/// An example image, drawn in the middle of the orbit.
const PAW: &[u8] = include_bytes!("../assets/paw.png");
const PAW_SIZE: f32 = 96.0;

/// The example's own sound: every fifth point plays it instead of the
/// framework's blip.
const CHIME: usize = 0;

/// The example's texts; see `Game::TEXTS`.
const TEXTS: &[Text] = &[
    (
        "menu.subtitle",
        "Build. Play. Record. Share.",
        "Bygg. Spela. Spela in. Dela.",
    ),
    ("example.title", "Your game here", "Ditt spel här"),
    ("example.level", "Level {level}", "Nivå {level}"),
    ("example.score", "Score {score}", "Poäng {score}"),
    ("example.finish", "Finish level", "Avsluta nivån"),
    (
        "example.help",
        "Space: +1 point   Enter: finish level   Esc: pause",
        "Mellanslag: +1 poäng   Enter: avsluta nivån   Esc: paus",
    ),
];

pub struct ExampleGame {
    level: u32,
    /// Game time in seconds, advanced only by `update`.
    time: f32,
    score: u32,
    done: bool,
    sounds: Vec<Sfx>,
    /// The mouse, kept from `handle` so that `draw` can show hover.
    pointer: Option<macroquad::math::Vec2>,
}

impl Game for ExampleGame {
    const TEXTS: &'static [Text] = TEXTS;
    const SOUNDS: &'static [&'static [u8]] = &[include_bytes!("../assets/chime.wav")];

    fn new(level: u32) -> Self {
        ExampleGame {
            level,
            time: 0.0,
            score: 0,
            done: false,
            sounds: Vec::new(),
            pointer: None,
        }
    }

    fn handle(&mut self, input: &Input) {
        self.pointer = input.pointer;
        if self.done {
            return;
        }
        if input.key(Key::Space) {
            self.score += 1;
            let sound = if self.score.is_multiple_of(5) {
                Sfx::Game(CHIME)
            } else {
                Sfx::Blip
            };
            self.sounds.push(sound);
        }
        if input.key(Key::Enter) || input.clicked(FINISH_BUTTON) {
            self.done = true;
        }
    }

    fn update(&mut self, dt: f32) {
        self.time += dt;
    }

    fn draw(&self, v: &View) {
        let cx = FIELD_W / 2.0;
        let level = t!("example.level", level = self.level);
        v.text(&level, 24.0, 48.0, 28.0, NEON);
        let score = t!("example.score", score = self.score);
        let w = v.text_width(&score, 28.0);
        v.text(&score, FIELD_W - 24.0 - w, 48.0, 28.0, ACCENT);

        let (ox, oy) = ORBIT_CENTRE;
        v.circle(vec2(ox, oy), ORBIT_R, with_alpha(NEON, 0.06));
        // An image from assets/, embedded in the program.
        let paw = PAW_SIZE;
        v.image(PAW, ox - paw / 2.0, oy - paw / 2.0, paw, paw, WHITE);
        let angle = self.time * std::f32::consts::TAU / 3.0;
        let dot = vec2(ox + ORBIT_R * angle.cos(), oy + ORBIT_R * angle.sin());
        v.circle(dot, 16.0, with_alpha(ACCENT, 0.3));
        v.circle(dot, 10.0, ACCENT);
        v.text_centered(t!("example.title"), cx, 300.0, 52.0, TEXT);

        let hover = self.pointer.is_some_and(|p| FINISH_BUTTON.contains(p));
        draw_button(v, FINISH_BUTTON, t!("example.finish"), hover, true);
        let help = with_alpha(TEXT, 0.7);
        v.text_wrapped_centered(t!("example.help"), 860.0, 20.0, help);
    }

    fn take_sounds(&mut self) -> Vec<Sfx> {
        std::mem::take(&mut self.sounds)
    }

    fn finished(&self) -> Option<RoundResult> {
        self.done.then_some(RoundResult { score: self.score })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key) -> Input {
        Input {
            keys: vec![k],
            ..Default::default()
        }
    }

    #[test]
    fn space_scores_and_enter_finishes() {
        let mut g = ExampleGame::new(3);
        g.handle(&key(Key::Space));
        g.handle(&key(Key::Space));
        assert_eq!(g.take_sounds(), vec![Sfx::Blip, Sfx::Blip]);
        assert!(g.take_sounds().is_empty(), "sounds are taken once");
        assert_eq!(g.finished(), None);
        g.handle(&key(Key::Enter));
        assert_eq!(g.finished(), Some(RoundResult { score: 2 }));
        g.handle(&key(Key::Space));
        assert_eq!(g.finished(), Some(RoundResult { score: 2 }), "frozen");
    }

    #[test]
    fn the_finish_button_finishes() {
        let mut g = ExampleGame::new(1);
        let click = Input {
            pointer: Some(FINISH_BUTTON.center()),
            left_pressed: true,
            left_down: true,
            ..Default::default()
        };
        g.handle(&click);
        assert!(g.finished().is_some());
    }

    #[test]
    fn time_only_advances_in_update() {
        let mut g = ExampleGame::new(1);
        for _ in 0..120 {
            g.update(crate::game::FIXED_DT);
        }
        assert!((g.time - 1.0).abs() < 1e-4);
    }
}
