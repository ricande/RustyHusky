//! Drawing of the framework's screens: main menu, settings, the running
//! game with its pause menu and REC marker, and the results. Uses the same
//! button functions as the input handling in `app.rs`, so what is drawn is
//! what is clicked.

use macroquad::prelude::*;

use crate::app::{
    menu_buttons, pause_buttons, results_buttons, settings_buttons, App, Screen, VideoStatus,
};
use crate::game::Game;
use crate::t;
use crate::ui::Button;
use crate::view::{
    dim, draw_background, draw_button, with_alpha, View, ACCENT, BACKDROP, ERROR, FIELD_W, NEON,
    TEXT,
};

/// Draws the current screen. `time` (seconds) drives blinking; `recording`
/// shows the REC marker, which only ever appears in the window.
pub fn draw<G: Game>(app: &App<G>, v: &View, pointer: Option<Vec2>, recording: bool, time: f32) {
    clear_background(BACKDROP);
    let blink = time.rem_euclid(1.0) < 0.6;
    match &app.screen {
        Screen::Menu => draw_menu(app, v, pointer),
        Screen::Settings => draw_settings(app, v, pointer),
        Screen::Playing { game, paused } => {
            draw_game(game, v);
            if recording {
                draw_rec_marker(v, blink || *paused);
            }
            if *paused {
                draw_pause(app, v, pointer);
            }
        }
        Screen::Results { level, result } => {
            draw_background(v);
            dim(v, 0.3);
            let cx = FIELD_W / 2.0;
            match result {
                Some(r) => {
                    let title = t!("results.complete", level = level);
                    v.text_centered(&title, cx, 220.0, 52.0, ACCENT);
                    let score = t!("results.score", score = r.score);
                    v.text_centered(&score, cx, 300.0, 32.0, TEXT);
                }
                None => v.text_centered(t!("results.aborted"), cx, 220.0, 52.0, ERROR),
            }
            let time = t!("results.time", time = seconds(app.play_time));
            v.text_centered(&time, cx, 345.0, 26.0, with_alpha(TEXT, 0.8));
            draw_video_status(&app.video, v);
            draw_buttons(
                v,
                &results_buttons(result.is_some(), &app.video),
                app.focus,
                pointer,
            );
            if matches!(app.video, VideoStatus::Ready { .. }) {
                let note = with_alpha(TEXT, 0.6);
                v.text_wrapped_centered(t!("results.leave_note"), 880.0, 17.0, note);
            }
        }
    }
}

/// The game on its background: the same for the window and the video.
pub fn draw_game<G: Game>(game: &G, v: &View) {
    draw_background(v);
    game.draw(v);
}

fn draw_buttons<A>(v: &View, buttons: &[Button<A>], focus: usize, pointer: Option<Vec2>) {
    for (i, b) in buttons.iter().enumerate() {
        let hot = i == focus || pointer.is_some_and(|p| b.rect.contains(p));
        draw_button(v, b.rect, &b.label, hot, b.enabled);
    }
}

fn draw_menu<G>(app: &App<G>, v: &View, pointer: Option<Vec2>) {
    draw_background(v);
    let cx = FIELD_W / 2.0;
    let title = crate::config::APP_NAME.to_uppercase();
    v.text_centered(&title, cx, 360.0, 76.0, ACCENT);
    v.text_centered(t!("menu.subtitle"), cx, 410.0, 24.0, NEON);
    draw_buttons(v, &menu_buttons(), app.focus, pointer);
    v.text_centered(t!("menu.help"), cx, 920.0, 18.0, with_alpha(TEXT, 0.6));
}

fn draw_settings<G>(app: &App<G>, v: &View, pointer: Option<Vec2>) {
    draw_background(v);
    let cx = FIELD_W / 2.0;
    v.text_centered(t!("settings.title"), cx, 150.0, 44.0, ACCENT);
    draw_buttons(v, &settings_buttons(&app.settings), app.focus, pointer);
    let mut y = 480.0;
    let note = with_alpha(TEXT, 0.75);
    y = v.text_wrapped_centered(t!("settings.recording_note"), y, 18.0, note);
    if let Some(why) = &app.recording_unavailable {
        y = v.text_wrapped_centered(why, y + 10.0, 18.0, ERROR);
    }
    if let Some(e) = &app.settings_error {
        v.text_wrapped_centered(e, y + 10.0, 18.0, ERROR);
    }
    v.text_centered(t!("settings.help"), cx, 920.0, 18.0, with_alpha(TEXT, 0.6));
}

fn draw_pause<G>(app: &App<G>, v: &View, pointer: Option<Vec2>) {
    dim(v, 0.65);
    let cx = FIELD_W / 2.0;
    v.text_centered(t!("pause.title"), cx, 400.0, 72.0, ACCENT);
    draw_buttons(v, &pause_buttons(), app.focus, pointer);
    v.text_centered(t!("pause.help"), cx, 640.0, 20.0, TEXT);
}

fn draw_rec_marker(v: &View, lit: bool) {
    let dot = if lit { ERROR } else { with_alpha(ERROR, 0.3) };
    v.circle(vec2(462.0, 82.0), 6.0, dot);
    v.text(t!("hud.rec"), 474.0, 89.0, 18.0, ERROR);
}

fn draw_video_status(video: &VideoStatus, v: &View) {
    let y = 450.0;
    let size = 19.0;
    let path;
    let (text, color): (&str, Color) = match video {
        VideoStatus::Off => (t!("video.status_off"), with_alpha(TEXT, 0.6)),
        VideoStatus::Unavailable(why) => (why, ERROR),
        VideoStatus::Recording => (t!("video.status_recording"), ACCENT),
        VideoStatus::Finishing => (t!("video.status_finishing"), ACCENT),
        VideoStatus::Ready { error: None } => (t!("video.status_ready"), NEON),
        VideoStatus::Ready { error: Some(e) } => (e, ERROR),
        VideoStatus::Saving => (t!("video.status_saving"), ACCENT),
        VideoStatus::Saved(p) => {
            path = t!("video.status_saved", path = crate::video::display_path(p));
            (&path, NEON)
        }
        VideoStatus::Discarded => (t!("video.status_discarded"), with_alpha(TEXT, 0.7)),
        VideoStatus::Failed(e) => (e, ERROR),
    };
    v.text_wrapped_centered(text, y, size, color);
}

/// Seconds with one decimal, with a decimal comma in Swedish.
fn seconds(t: f32) -> String {
    let s = format!("{t:.1}");
    if crate::i18n::lang() == crate::i18n::Lang::Sv {
        s.replace('.', ",")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn seconds_follow_the_language() {
        assert_eq!(super::seconds(12.345), "12.3");
        std::thread::spawn(|| {
            crate::i18n::set_lang(crate::i18n::Lang::Sv);
            assert_eq!(super::seconds(12.345), "12,3");
        })
        .join()
        .unwrap();
    }
}
