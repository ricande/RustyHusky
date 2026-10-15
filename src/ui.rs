//! Window-independent input and buttons. Everything is in logical
//! coordinates, so input handling and drawing agree on where things are
//! however the window is sized or letterboxed.

use macroquad::math::{Rect, Vec2};

/// The keys the program reads. To use another key, add it here, to
/// `Key::ALL` and to the table in `main.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    W,
    A,
    S,
    D,
    Z,
    X,
    P,
    Space,
    Enter,
    Escape,
}

impl Key {
    pub const ALL: [Key; 14] = [
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
        Key::W,
        Key::A,
        Key::S,
        Key::D,
        Key::Z,
        Key::X,
        Key::P,
        Key::Space,
        Key::Enter,
        Key::Escape,
    ];

    /// The key's name in scripts: `up`, `space`, `enter`, `esc`, `p` …
    pub fn name(self) -> &'static str {
        match self {
            Key::Up => "up",
            Key::Down => "down",
            Key::Left => "left",
            Key::Right => "right",
            Key::W => "w",
            Key::A => "a",
            Key::S => "s",
            Key::D => "d",
            Key::Z => "z",
            Key::X => "x",
            Key::P => "p",
            Key::Space => "space",
            Key::Enter => "enter",
            Key::Escape => "esc",
        }
    }

    pub fn from_name(name: &str) -> Option<Key> {
        let name = name.to_lowercase();
        Key::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// One frame of input, already converted to logical coordinates.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// Mouse position in logical coordinates; `None` outside the field.
    pub pointer: Option<Vec2>,
    /// The left button went down this frame.
    pub left_pressed: bool,
    /// The left button is held.
    pub left_down: bool,
    /// Keys pressed this frame.
    pub keys: Vec<Key>,
    /// Keys held down this frame (including those just pressed).
    pub held: Vec<Key>,
}

impl Input {
    /// `key` was pressed this frame.
    pub fn key(&self, key: Key) -> bool {
        self.keys.contains(&key)
    }

    /// `key` is held down (e.g. for movement in a game).
    #[allow(dead_code)] // Part of the game API; the example does not need it.
    pub fn held(&self, key: Key) -> bool {
        self.held.contains(&key)
    }

    /// The left button was pressed on `rect` this frame.
    pub fn clicked(&self, rect: Rect) -> bool {
        self.left_pressed && self.pointer.is_some_and(|p| rect.contains(p))
    }
}

#[derive(Clone, Debug)]
pub struct Button<A> {
    pub rect: Rect,
    pub label: String,
    pub action: A,
    pub enabled: bool,
}

/// Width and height of the standard menu button.
pub const BUTTON_W: f32 = 300.0;
pub const BUTTON_H: f32 = 52.0;
const SPACING: f32 = 12.0;

/// A centred column of standard buttons starting at `top`.
pub fn button_column<A>(top: f32, items: Vec<(String, A)>) -> Vec<Button<A>> {
    let x = (crate::view::FIELD_W - BUTTON_W) / 2.0;
    items
        .into_iter()
        .enumerate()
        .map(|(i, (label, action))| Button {
            rect: Rect::new(x, top + i as f32 * (BUTTON_H + SPACING), BUTTON_W, BUTTON_H),
            label,
            action,
            enabled: true,
        })
        .collect()
}

/// A row of equal buttons filling `area`.
pub fn button_row<A>(area: Rect, items: Vec<(String, A)>) -> Vec<Button<A>> {
    let n = items.len() as f32;
    let w = (area.w - SPACING * (n - 1.0)) / n;
    items
        .into_iter()
        .enumerate()
        .map(|(i, (label, action))| Button {
            rect: Rect::new(area.x + i as f32 * (w + SPACING), area.y, w, area.h),
            label,
            action,
            enabled: true,
        })
        .collect()
}

/// Mouse and keyboard handling for a list of buttons. Up/Down move the
/// focus (skipping disabled buttons), hovering moves it too, and a click or
/// Enter/Space chooses. `focus` is an index into `buttons`.
pub fn choose<A: Copy>(buttons: &[Button<A>], focus: &mut usize, input: &Input) -> Option<A> {
    let enabled: Vec<usize> = (0..buttons.len()).filter(|&i| buttons[i].enabled).collect();
    let last = *enabled.last()?;
    if !enabled.contains(focus) {
        *focus = enabled
            .iter()
            .copied()
            .find(|&i| i >= *focus)
            .unwrap_or(last);
    }
    let pos = enabled.iter().position(|&i| i == *focus).unwrap_or(0);
    if input.key(Key::Up) {
        *focus = enabled[pos.saturating_sub(1)];
    }
    if input.key(Key::Down) {
        *focus = enabled[(pos + 1).min(enabled.len() - 1)];
    }
    if let Some(p) = input.pointer {
        if let Some(&i) = enabled.iter().find(|&&i| buttons[i].rect.contains(p)) {
            *focus = i;
            if input.left_pressed {
                return Some(buttons[i].action);
            }
        }
    }
    (input.key(Key::Enter) || input.key(Key::Space)).then(|| buttons[*focus].action)
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

    fn buttons() -> Vec<Button<u8>> {
        let mut b = button_column(
            100.0,
            vec![("a".into(), 1), ("b".into(), 2), ("c".into(), 3)],
        );
        b[1].enabled = false;
        b
    }

    #[test]
    fn keyboard_skips_disabled_buttons() {
        let b = buttons();
        let mut focus = 0;
        assert_eq!(choose(&b, &mut focus, &key(Key::Down)), None);
        assert_eq!(focus, 2, "b is disabled");
        assert_eq!(choose(&b, &mut focus, &key(Key::Down)), None);
        assert_eq!(focus, 2, "stays at the end");
        assert_eq!(choose(&b, &mut focus, &key(Key::Enter)), Some(3));
        choose(&b, &mut focus, &key(Key::Up));
        assert_eq!(focus, 0);
        // A focus on a disabled button moves to the next enabled one.
        let mut focus = 1;
        assert_eq!(choose(&b, &mut focus, &key(Key::Space)), Some(3));
    }

    #[test]
    fn clicks_choose_only_enabled_buttons() {
        let b = buttons();
        let click = |i: usize| Input {
            pointer: Some(b[i].rect.center()),
            left_pressed: true,
            left_down: true,
            ..Default::default()
        };
        let mut focus = 0;
        assert_eq!(choose(&b, &mut focus, &click(2)), Some(3));
        assert_eq!(focus, 2);
        assert_eq!(choose(&b, &mut focus, &click(1)), None, "disabled");
        let hover = Input {
            pointer: Some(b[0].rect.center()),
            ..Default::default()
        };
        assert_eq!(choose(&b, &mut focus, &hover), None);
        assert_eq!(focus, 0, "hovering moves the focus");
        let none: Vec<Button<u8>> = Vec::new();
        assert_eq!(choose(&none, &mut focus, &key(Key::Enter)), None);
    }

    #[test]
    fn layout_helpers_fill_their_area() {
        let row = button_row(
            Rect::new(20.0, 0.0, 500.0, 40.0),
            vec![("a".into(), 1), ("b".into(), 2)],
        );
        assert_eq!(row[0].rect.x, 20.0);
        assert!((row[1].rect.right() - 520.0).abs() < 0.01);
        let col = button_column(100.0, vec![("a".into(), 1), ("b".into(), 2)]);
        assert!(col[1].rect.y > col[0].rect.bottom());
        assert!((col[0].rect.center().x - crate::view::FIELD_W / 2.0).abs() < 0.01);
    }

    #[test]
    fn key_names_round_trip() {
        for k in Key::ALL {
            assert_eq!(Key::from_name(k.name()), Some(k));
        }
        assert_eq!(Key::from_name("ESC"), Some(Key::Escape));
        assert_eq!(Key::from_name("hyper"), None);
    }
}
