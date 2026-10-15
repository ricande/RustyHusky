//! The logical 540 × 960 field and how it is mapped onto the window (or a
//! video frame), plus small drawing helpers that take logical coordinates.
//!
//! Everything else in the program – the game, menus, buttons, mouse input –
//! works in logical coordinates. Only `View` knows about real pixels.

use macroquad::prelude::*;

/// Logical width of the field (portrait 9:16).
pub const FIELD_W: f32 = 540.0;
/// Logical height of the field.
pub const FIELD_H: f32 = 960.0;

/// Colour of the window outside the field (the letterbox bars).
pub const BACKDROP: Color = Color::new(0.02, 0.02, 0.05, 1.0);
const FIELD_TOP: Color = Color::new(0.06, 0.05, 0.16, 1.0);
const FIELD_BOTTOM: Color = Color::new(0.02, 0.07, 0.12, 1.0);
pub const NEON: Color = Color::new(0.35, 0.85, 1.0, 1.0);
pub const TEXT: Color = Color::new(0.95, 0.95, 1.0, 1.0);
pub const ACCENT: Color = Color::new(1.0, 0.8, 0.25, 1.0);
pub const ERROR: Color = Color::new(1.0, 0.4, 0.4, 1.0);
const BUTTON: Color = Color::new(0.08, 0.10, 0.22, 1.0);
const BUTTON_HOT: Color = Color::new(0.14, 0.20, 0.40, 1.0);

pub fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Maps the fixed 540 × 960 field onto a target of any size, letterboxed so
/// the proportions never change: the largest 9:16 area that fits, centred.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub scale: f32,
    ox: f32,
    oy: f32,
}

impl View {
    pub fn fit(target_w: f32, target_h: f32) -> Self {
        let scale = (target_w / FIELD_W).min(target_h / FIELD_H).max(0.01);
        View {
            scale,
            ox: (target_w - FIELD_W * scale) / 2.0,
            oy: (target_h - FIELD_H * scale) / 2.0,
        }
    }

    /// Converts a window position (e.g. the mouse) to logical coordinates.
    pub fn to_field(self, window: (f32, f32)) -> Vec2 {
        vec2(
            (window.0 - self.ox) / self.scale,
            (window.1 - self.oy) / self.scale,
        )
    }

    /// The logical position of a window position, or `None` if it lies in
    /// the letterbox bars outside the field.
    pub fn pointer(self, window: (f32, f32)) -> Option<Vec2> {
        let p = self.to_field(window);
        ((0.0..=FIELD_W).contains(&p.x) && (0.0..=FIELD_H).contains(&p.y)).then_some(p)
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let s = self.scale;
        draw_rectangle(self.ox + x * s, self.oy + y * s, w * s, h * s, color);
    }

    pub fn rect_lines(&self, x: f32, y: f32, w: f32, h: f32, thick: f32, color: Color) {
        let s = self.scale;
        draw_rectangle_lines(
            self.ox + x * s,
            self.oy + y * s,
            w * s,
            h * s,
            thick * s,
            color,
        );
    }

    pub fn circle(&self, p: Vec2, r: f32, color: Color) {
        draw_circle(
            self.ox + p.x * self.scale,
            self.oy + p.y * self.scale,
            r * self.scale,
            color,
        );
    }

    pub fn line(&self, a: Vec2, b: Vec2, thick: f32, color: Color) {
        let s = self.scale;
        draw_line(
            self.ox + a.x * s,
            self.oy + a.y * s,
            self.ox + b.x * s,
            self.oy + b.y * s,
            thick * s,
            color,
        );
    }

    /// Width of `text` at `size`, in logical units.
    pub fn text_width(&self, text: &str, size: f32) -> f32 {
        let px = (size * self.scale).round().max(1.0) as u16;
        measure_text(text, None, px, 1.0).width / self.scale
    }

    /// Draws text with a drop shadow; `y` is the baseline.
    pub fn text(&self, text: &str, x: f32, y: f32, size: f32, color: Color) {
        let px = (size * self.scale).round().max(1.0);
        let sx = self.ox + x * self.scale;
        let sy = self.oy + y * self.scale;
        let off = (size * self.scale * 0.06).max(1.0);
        let shadow = with_alpha(BLACK, 0.7 * color.a);
        draw_text(text, sx + off, sy + off, px, shadow);
        draw_text(text, sx, sy, px, color);
    }

    /// Centred text, shrunk if needed so it never spills outside the field.
    pub fn text_centered(&self, text: &str, cx: f32, y: f32, size: f32, color: Color) {
        const MAX_W: f32 = FIELD_W - 32.0;
        let mut size = size;
        let mut w = self.text_width(text, size);
        if w > MAX_W {
            size *= MAX_W / w;
            w = self.text_width(text, size);
        }
        self.text(text, cx - w / 2.0, y, size, color);
    }

    /// Text wrapped to lines that fit the field, every line centred.
    /// Returns the baseline below the last line.
    pub fn text_wrapped_centered(&self, text: &str, y: f32, size: f32, color: Color) -> f32 {
        let width = ((FIELD_W - 64.0) / (size * 0.47)) as usize;
        let mut y = y;
        for line in wrap(text, width.max(10)) {
            self.text_centered(&line, FIELD_W / 2.0, y, size, color);
            y += size * 1.3;
        }
        y
    }
}

/// Splits text into lines of at most `width` characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let mut word: Vec<char> = word.chars().collect();
            // Hard-break very long words such as file paths.
            while word.len() > width {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                lines.push(word.drain(..width).collect());
            }
            let word: String = word.into_iter().collect();
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&word);
        }
        lines.push(line);
    }
    lines
}

/// Ends a batch of drawing with a draw that does not use the font atlas.
/// Call last before the drawing is flushed (camera change, end of frame).
///
/// Works around miniquad 0.4.11: `delete_texture` does not clear its
/// texture-binding cache. When the font atlas grows, macroquad deletes its
/// texture and creates a new one that often gets the same GL name; if the
/// cache still believes that name is bound, the new texture's pixels are
/// never uploaded and all text turns into black blocks for good. Leaving a
/// plain (white-texture) draw last means the atlas is never the cached
/// binding when that happens.
pub fn end_batch() {
    draw_rectangle(0.0, 0.0, 1.0, 1.0, Color::new(0.0, 0.0, 0.0, 0.0));
}

/// The default field background: a dark vertical gradient with a faint grid.
pub fn draw_background(v: &View) {
    const BANDS: usize = 24;
    let band_h = FIELD_H / BANDS as f32;
    for i in 0..BANDS {
        let t = i as f32 / (BANDS - 1) as f32;
        let color = lerp_color(FIELD_TOP, FIELD_BOTTOM, t);
        v.rect(0.0, i as f32 * band_h, FIELD_W, band_h + 0.5, color);
    }
    let grid = with_alpha(NEON, 0.05);
    let mut x = 0.0;
    while x <= FIELD_W {
        v.line(vec2(x, 0.0), vec2(x, FIELD_H), 1.0, grid);
        x += 40.0;
    }
    let mut y = 0.0;
    while y <= FIELD_H {
        v.line(vec2(0.0, y), vec2(FIELD_W, y), 1.0, grid);
        y += 40.0;
    }
}

/// Darkens the whole field, e.g. behind a menu.
pub fn dim(v: &View, alpha: f32) {
    v.rect(0.0, 0.0, FIELD_W, FIELD_H, with_alpha(BLACK, alpha));
}

/// A button: `hot` when focused or hovered; disabled buttons are faded.
pub fn draw_button(v: &View, rect: Rect, label: &str, hot: bool, enabled: bool) {
    let alpha = if enabled { 1.0 } else { 0.35 };
    let bg = if hot && enabled { BUTTON_HOT } else { BUTTON };
    v.rect(rect.x, rect.y, rect.w, rect.h, with_alpha(bg, alpha));
    let border = with_alpha(NEON, if hot && enabled { 1.0 } else { 0.5 * alpha });
    v.rect_lines(rect.x, rect.y, rect.w, rect.h, 2.0, border);
    // Shrink a long label to fit the button.
    let mut size = (rect.h * 0.5).min(28.0);
    let room = rect.w - 24.0;
    let width = v.text_width(label, size);
    if width > room {
        size *= room / width;
    }
    let cx = rect.x + rect.w / 2.0;
    let baseline = rect.y + rect.h / 2.0 + size * 0.32;
    v.text_centered(label, cx, baseline, size, with_alpha(TEXT, alpha));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_keeps_9_16_and_centres_in_any_window() {
        for (sw, sh) in [
            (540.0, 960.0),
            (1080.0, 1920.0),
            (1920.0, 1080.0),
            (300.0, 900.0),
            (1000.0, 640.0),
            (800.0, 600.0),
        ] {
            let v = View::fit(sw, sh);
            let (w, h) = (FIELD_W * v.scale, FIELD_H * v.scale);
            assert!((w / h - 9.0 / 16.0).abs() < 1e-4, "never stretched");
            assert!(w <= sw + 0.01 && h <= sh + 0.01, "never cropped");
            assert!((v.ox * 2.0 + w - sw).abs() < 0.01, "centred horizontally");
            assert!((v.oy * 2.0 + h - sh).abs() < 0.01, "centred vertically");
            assert!(v.ox.abs() < 0.01 || v.oy.abs() < 0.01, "fills one axis");
        }
        // The video frame is exactly twice the logical size, with no bars.
        let video = View::fit(1080.0, 1920.0);
        assert_eq!((video.scale, video.ox, video.oy), (2.0, 0.0, 0.0));
    }

    #[test]
    fn window_positions_map_to_logical_coordinates() {
        // 1920 × 1080: the field is 607.5 × 1080, starting at x = 656.25.
        let v = View::fit(1920.0, 1080.0);
        let centre = v.to_field((960.0, 540.0));
        assert!((centre - vec2(FIELD_W / 2.0, FIELD_H / 2.0)).length() < 0.01);
        assert!(v.to_field((656.25, 0.0)).length() < 0.01, "top left corner");
        let corner = v.to_field((656.25 + 607.5, 1080.0));
        assert!((corner - vec2(FIELD_W, FIELD_H)).length() < 0.01);
        assert!(v.pointer((960.0, 540.0)).is_some());
        assert_eq!(v.pointer((100.0, 540.0)), None, "left letterbox");
        assert_eq!(v.pointer((1800.0, 540.0)), None, "right letterbox");
        // A tall window has bars above and below instead.
        let v = View::fit(540.0, 1200.0);
        assert_eq!(v.pointer((270.0, 60.0)), None, "top bar");
        assert_eq!(v.pointer((270.0, 120.0)), Some(vec2(270.0, 0.0)));
        // A window twice the logical size: positions halve.
        let v = View::fit(1080.0, 1920.0);
        assert_eq!(v.to_field((540.0, 1000.0)), vec2(270.0, 500.0));
    }

    #[test]
    fn wrap_breaks_words_and_long_paths() {
        assert_eq!(wrap("aa bb cc", 5), vec!["aa bb", "cc"]);
        assert_eq!(
            wrap("x /a/very/long/path", 6),
            vec!["x", "/a/ver", "y/long", "/path"]
        );
        assert_eq!(wrap("rad1\nrad2", 20), vec!["rad1", "rad2"]);
    }
}
