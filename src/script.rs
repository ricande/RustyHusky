//! Scripted runs, for checking the UI and the recording without touching
//! the real mouse and keyboard: `RUSTYHUSKY_SCRIPT=check.txt cargo run` (the prefix is `config::ENV_PREFIX`).
//!
//! A script is a text file with one step per line, `<frame> <command> …`.
//! Frames count from 1 and steps run in the frame they name; `#` starts a
//! comment. In a scripted run the frame time is a fixed 1/60 s, so the same
//! script gives the same pictures.
//!
//! | Command              | Effect                                                 |
//! | -------------------- | ------------------------------------------------------ |
//! | `key <name>`         | press a key (`enter`, `esc`, `space`, `up`, `p` …)     |
//! | `hold <name>`        | press a key and keep it held in the following frames   |
//! | `release <name>`     | let go of a held key                                   |
//! | `click <x> <y>`      | left click at a logical position (540 × 960)           |
//! | `wclick <x> <y>`     | left click at a window pixel, converted like the mouse |
//! | `move <x> <y>`       | move the mouse to a logical position                   |
//! | `shot <name>`        | save the window as `<name>.png` in `RUSTYHUSKY_SHOTS`  |
//! | `quit`               | end the program after this frame                       |

use macroquad::math::Vec2;

use crate::ui::{Input, Key};

#[derive(Clone, Debug, PartialEq)]
enum Command {
    Key(Key),
    Hold(Key),
    Release(Key),
    Click(Vec2),
    WindowClick(f32, f32),
    Move(Vec2),
    Shot(String),
    Quit,
}

/// What a scripted frame asks for.
#[derive(Debug, Default)]
pub struct Frame {
    pub input: Input,
    /// Pictures to save after this frame is drawn.
    pub shots: Vec<String>,
    pub quit: bool,
}

#[derive(Debug, Default)]
pub struct Script {
    steps: Vec<(u32, Command)>,
    pointer: Option<Vec2>,
    /// Keys held with `hold`.
    held: Vec<Key>,
}

fn parse_pos(args: &[&str]) -> Result<(f32, f32), String> {
    match args {
        [x, y] => {
            let x: f32 = x.parse().map_err(|_| format!("bad x: {x}"))?;
            let y: f32 = y.parse().map_err(|_| format!("bad y: {y}"))?;
            Ok((x, y))
        }
        _ => Err("expected <x> <y>".into()),
    }
}

impl Script {
    /// Parses a script; errors name the line.
    pub fn parse(text: &str) -> Result<Script, String> {
        let mut steps = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let fail = |msg: String| format!("line {}: {msg}: {raw}", i + 1);
            let mut words = line.split_whitespace();
            let frame: u32 = words
                .next()
                .and_then(|w| w.parse().ok())
                .filter(|&f| f > 0)
                .ok_or_else(|| fail("expected a frame number from 1".into()))?;
            let name = words.next().ok_or_else(|| fail("missing command".into()))?;
            let args: Vec<&str> = words.collect();
            let pos = || parse_pos(&args).map_err(fail);
            let command = match (name, args.as_slice()) {
                ("key" | "hold" | "release", [k]) => {
                    let key = Key::from_name(k).ok_or_else(|| fail(format!("unknown key {k}")))?;
                    match name {
                        "key" => Command::Key(key),
                        "hold" => Command::Hold(key),
                        _ => Command::Release(key),
                    }
                }
                ("click", _) => Command::Click(pos()?.into()),
                ("wclick", _) => {
                    let (x, y) = pos()?;
                    Command::WindowClick(x, y)
                }
                ("move", _) => Command::Move(pos()?.into()),
                ("shot", [n]) => Command::Shot((*n).to_owned()),
                ("quit", []) => Command::Quit,
                _ => return Err(fail("unknown command or wrong arguments".into())),
            };
            steps.push((frame, command));
        }
        Ok(Script {
            steps,
            ..Default::default()
        })
    }

    /// The input and actions for frame `n` (counting from 1). `to_field`
    /// converts a window pixel to a logical position (`None` outside).
    pub fn frame(&mut self, n: u32, to_field: impl Fn((f32, f32)) -> Option<Vec2>) -> Frame {
        let mut f = Frame::default();
        for (_, command) in self.steps.iter().filter(|(at, _)| *at == n) {
            match command {
                Command::Key(k) => {
                    f.input.keys.push(*k);
                    f.input.held.push(*k);
                }
                Command::Hold(k) => {
                    f.input.keys.push(*k);
                    if !self.held.contains(k) {
                        self.held.push(*k);
                    }
                }
                Command::Release(k) => self.held.retain(|h| h != k),
                Command::Click(p) => {
                    self.pointer = Some(*p);
                    f.input.left_pressed = true;
                }
                Command::WindowClick(x, y) => {
                    self.pointer = to_field((*x, *y));
                    f.input.left_pressed = true;
                }
                Command::Move(p) => self.pointer = Some(*p),
                Command::Shot(name) => f.shots.push(name.clone()),
                Command::Quit => f.quit = true,
            }
        }
        for k in &self.held {
            if !f.input.held.contains(k) {
                f.input.held.push(*k);
            }
        }
        f.input.pointer = self.pointer;
        f.input.left_down = f.input.left_pressed;
        f
    }

    /// The last frame any step refers to.
    pub fn last_frame(&self) -> u32 {
        self.steps.iter().map(|(n, _)| *n).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::math::vec2;

    fn no_window(_: (f32, f32)) -> Option<Vec2> {
        None
    }

    #[test]
    fn steps_become_input_in_their_frame() {
        let mut s = Script::parse(
            "# a comment\n\
             2 key enter\n\
             2 key p   # same frame\n\
             4 click 100 200\n\
             6 shot menu\n\
             7 quit\n",
        )
        .unwrap();
        assert_eq!(s.last_frame(), 7);
        assert!(s.frame(1, no_window).input.keys.is_empty());
        assert_eq!(s.frame(2, no_window).input.keys, vec![Key::Enter, Key::P]);
        let click = s.frame(4, no_window).input;
        assert!(click.left_pressed && click.left_down);
        assert_eq!(click.pointer, Some(vec2(100.0, 200.0)));
        let after = s.frame(5, no_window).input;
        assert!(
            !after.left_pressed && !after.left_down,
            "a click is released"
        );
        assert_eq!(after.pointer, Some(vec2(100.0, 200.0)), "the mouse stays");
        assert_eq!(s.frame(6, no_window).shots, vec!["menu"]);
        assert!(s.frame(7, no_window).quit);
    }

    #[test]
    fn held_keys_stay_down_until_released() {
        let mut s = Script::parse("1 hold left\n3 release left\n4 key right").unwrap();
        let f = s.frame(1, no_window).input;
        assert!(f.key(Key::Left) && f.held(Key::Left), "pressed and held");
        let f = s.frame(2, no_window).input;
        assert!(!f.key(Key::Left) && f.held(Key::Left), "still held");
        assert!(!s.frame(3, no_window).input.held(Key::Left), "released");
        let f = s.frame(4, no_window).input;
        assert!(f.key(Key::Right) && f.held(Key::Right) && f.held.len() == 1);
    }

    #[test]
    fn window_clicks_are_converted() {
        let mut s = Script::parse("1 wclick 500 320\n2 wclick 10 10").unwrap();
        let view = crate::view::View::fit(1000.0, 640.0);
        let f = s.frame(1, |p| view.pointer(p));
        assert_eq!(f.input.pointer, Some(vec2(270.0, 480.0)), "window centre");
        let f = s.frame(2, |p| view.pointer(p));
        assert!(
            f.input.left_pressed && f.input.pointer.is_none(),
            "in the side bar"
        );
    }

    #[test]
    fn errors_name_the_line() {
        for (text, expected) in [
            ("0 quit", "line 1"),
            ("x quit", "frame number"),
            ("1 quit\n2 jump", "line 2"),
            ("1 key hyper", "unknown key"),
            ("1 click 10", "<x> <y>"),
            ("1 click a 10", "bad x"),
            ("1 quit now", "wrong arguments"),
        ] {
            let err = Script::parse(text).unwrap_err();
            assert!(err.contains(expected), "{text}: {err}");
        }
    }
}
