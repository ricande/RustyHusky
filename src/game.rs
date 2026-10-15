//! The interface between the framework and a game.
//!
//! The framework (menus, settings, pause, results, scaling, recording)
//! drives one round at a time through this trait. A new game implements it
//! in its own module and is selected with `ActiveGame` in `main.rs`; nothing
//! else has to change. See `example.rs` for a minimal implementation.

use crate::audio::Sfx;
use crate::ui::Input;
use crate::view::View;

/// Fixed simulation time step: `Game::update` is always called with this.
pub const FIXED_DT: f32 = 1.0 / 120.0;

/// What a finished level reports to the results screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoundResult {
    pub score: u32,
}

pub trait Game {
    /// A new round. `level` is 1 for the first round after the main menu and
    /// grows by one for each "Next round".
    fn new(level: u32) -> Self
    where
        Self: Sized;

    /// Called once per displayed frame while the round runs (not while
    /// paused), with that frame's input in logical coordinates. Esc and P
    /// are used by the framework to pause.
    fn handle(&mut self, input: &Input);

    /// Advances the game by `dt` (always `FIXED_DT`) seconds. Called zero
    /// or more times per frame, so the game runs at the same speed at any
    /// frame rate. Keep game time here rather than reading the clock, so the
    /// video matches what was played.
    fn update(&mut self, dt: f32);

    /// Draws the game in logical coordinates through `view`. The framework
    /// has already drawn the default background. Called for the window and,
    /// while recording, again for the video frame, so it must not change
    /// the game.
    fn draw(&self, view: &View);

    /// Sound effects started since the last call. The framework plays them
    /// and adds them to the video's sound track.
    fn take_sounds(&mut self) -> Vec<Sfx>;

    /// `Some` once the level is finished. The framework then stops
    /// recording, stops calling `handle`/`update` and shows the results.
    fn finished(&self) -> Option<RoundResult>;
}
