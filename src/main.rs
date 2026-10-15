//! A portrait game built on the RustyHusky starter framework.
//!
//! This file owns the window and the frame loop: it reads input, runs the
//! app at a fixed time step, plays sounds, feeds the recorder and draws.
//! The game itself is `ActiveGame`; see `game.rs` and the README.

mod app;
mod audio;
mod capture;
mod config;
mod example;
mod game;
mod i18n;
mod screen;
mod screens;
mod script;
mod settings;
mod ui;
mod video;
mod view;

use macroquad::prelude::*;

use app::App;
use game::FIXED_DT;
use ui::{Input, Key};
use view::View;

/// The game the framework runs. Replace it with your own type that
/// implements `game::Game`.
type ActiveGame = example::ExampleGame;

/// Upper bound on simulated time per frame, so a hitch never causes a
/// burst of catch-up steps.
const MAX_FRAME_TIME: f32 = 0.25;

/// `<ENV_PREFIX>_WINDOW=1000x640` forces a window size (e.g. to check
/// letterboxing).
fn forced_window_size() -> Option<(i32, i32)> {
    let value = config::env_str("WINDOW")?;
    let (w, h) = value.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn window_conf() -> Conf {
    let (width, height) = forced_window_size()
        .unwrap_or_else(|| screen::initial_window_size(screen::detect_work_area()));
    Conf {
        window_title: config::APP_NAME.to_owned(),
        window_width: width,
        window_height: height,
        window_resizable: true,
        sample_count: 4,
        ..Default::default()
    }
}

/// Collects this frame's input, with the mouse mapped to logical coordinates.
fn read_input(view: View) -> Input {
    let mut keys: Vec<Key> = ui::KEYS
        .iter()
        .filter(|(_, code, _)| is_key_pressed(*code))
        .map(|&(key, _, _)| key)
        .collect();
    let mut held: Vec<Key> = ui::KEYS
        .iter()
        .filter(|(_, code, _)| is_key_down(*code))
        .map(|&(key, _, _)| key)
        .collect();
    if is_key_pressed(KeyCode::KpEnter) {
        keys.push(Key::Enter);
        held.push(Key::Enter);
    }
    Input {
        pointer: view.pointer(mouse_position()),
        left_pressed: is_mouse_button_pressed(MouseButton::Left),
        left_down: is_mouse_button_down(MouseButton::Left),
        keys,
        held,
    }
}

/// The script from `<ENV_PREFIX>_SCRIPT`, if any. A broken script stops the
/// program with a message rather than running unscripted.
fn load_script() -> Option<script::Script> {
    let path = config::env("SCRIPT")?;
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("cannot read script {}: {e}", path.to_string_lossy());
        std::process::exit(2)
    });
    script::Script::parse(&text).map(Some).unwrap_or_else(|e| {
        eprintln!("script error: {e}");
        std::process::exit(2)
    })
}

/// Saves the window as `<<ENV_PREFIX>_SHOTS or "shots">/<name>.png`.
fn save_shot(name: &str) {
    let dir = config::env_str("SHOTS").unwrap_or_else(|| "shots".into());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {dir}: {e}");
        return;
    }
    let path = format!("{dir}/{name}.png");
    let mut image = get_screen_data();
    // The window's alpha channel is not what is shown; make it opaque.
    for alpha in image.bytes.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }
    image.export_png(&path);
    eprintln!("saved {path}");
}

#[macroquad::main(window_conf)]
async fn main() {
    // Closing the window ends the loop normally, so recordings are cleaned up.
    prevent_quit();
    let sounds = audio::Sounds::load().await;
    let settings_path = settings::default_path();
    let settings = settings::load(&settings_path);
    i18n::set_lang(settings::env_lang().unwrap_or(settings.lang()));
    let mut app: App<ActiveGame> = App::new(settings, Some(settings_path));
    let mut capture = capture::Capture::new(sounds.samples().to_vec(), &mut app);
    let mut script = load_script();
    let mut accumulator = 0.0;
    let mut time = 0.0;
    let mut frame_number = 0u32;

    loop {
        frame_number += 1;
        let view = View::fit(screen_width(), screen_height());
        let scripted = script
            .as_mut()
            .map(|s| s.frame(frame_number, |p| view.pointer(p)));
        let input = match &scripted {
            Some(f) => f.input.clone(),
            None => read_input(view),
        };

        app.handle(&input);

        // Scripted runs use a fixed frame time, so they are repeatable.
        let frame_time = if scripted.is_some() {
            1.0 / 60.0
        } else {
            get_frame_time().min(MAX_FRAME_TIME)
        };
        time += frame_time;
        accumulator += frame_time;
        while accumulator >= FIXED_DT {
            app.update(FIXED_DT);
            accumulator -= FIXED_DT;
        }

        let sfx = app.take_sounds();
        for &s in &sfx {
            sounds.play(s, app.settings.volume);
        }
        // Records this frame of the game (drawn off screen) before the
        // window is drawn, and stops at the end of the level.
        capture.frame(&mut app, frame_time, &sfx);
        app.end_round_if_finished();

        screens::draw(&app, &view, input.pointer, capture.is_recording(), time);

        let mut quit = app.quit || is_quit_requested();
        if let Some(f) = &scripted {
            for name in &f.shots {
                save_shot(name);
            }
            let past_end = script
                .as_ref()
                .is_some_and(|s| frame_number > s.last_frame());
            quit |= f.quit || past_end;
        }
        if quit {
            break;
        }
        next_frame().await;
    }
    // Stops ffmpeg and removes temporary files.
    drop(capture);
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    /// A fresh folder for one test under the system temp folder.
    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{}-test-{}-{name}",
            crate::config::APP_ID,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}
