//! Picks a start window size that fits the screen's usable work area while
//! keeping the playfield's 9:16 proportions.
//!
//! Macroquad cannot report the monitor size before the window exists, so on
//! X11 (including XWayland) the work area is read with `xprop`/`xrandr`.
//! Elsewhere, or if that fails, a conservative size that fits small screens
//! is used. The window can always be resized afterwards.

use std::process::Command;

use crate::view::{FIELD_H, FIELD_W};

/// Used when the work area is unknown: fits a 1366x768 laptop screen.
const FALLBACK: (i32, i32) = (360, 640);
/// Room left for the title bar and window borders.
const DECORATION_H: i32 = 48;
const MARGIN: i32 = 16;
const MIN_HEIGHT: i32 = 320;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Area {
    fn intersect(self, other: Area) -> Option<Area> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        (x1 > x0 && y1 > y0).then(|| Area {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }
}

/// Largest 9:16 window that fits inside the work area (`None` = unknown).
pub fn initial_window_size(work_area: Option<Area>) -> (i32, i32) {
    let Some(area) = work_area else {
        return FALLBACK;
    };
    let max_w = (area.w - 2 * MARGIN) as f32;
    let max_h = (area.h - DECORATION_H - 2 * MARGIN) as f32;
    let scale = (max_w / FIELD_W).min(max_h / FIELD_H);
    let h = ((FIELD_H * scale) as i32).max(MIN_HEIGHT);
    let w = (h as f32 * FIELD_W / FIELD_H).round() as i32;
    (w, h)
}

/// Best guess of the usable desktop area on the primary monitor.
pub fn detect_work_area() -> Option<Area> {
    let work = run("xprop", &["-root", "_NET_WORKAREA"]).and_then(|s| parse_workarea(&s));
    let primary = run("xrandr", &["--current"]).and_then(|s| parse_primary(&s));
    match (work, primary) {
        // On multi-monitor setups the work area spans every screen.
        (Some(w), Some(p)) => w.intersect(p).or(Some(p)),
        (w, p) => w.or(p),
    }
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parses `_NET_WORKAREA(CARDINAL) = 0, 0, 3440, 1386` (first desktop).
fn parse_workarea(s: &str) -> Option<Area> {
    let nums: Vec<i32> = s
        .split_once('=')?
        .1
        .split(',')
        .map(|n| n.trim().parse().ok())
        .collect::<Option<_>>()?;
    match nums[..] {
        [x, y, w, h, ..] if w > 0 && h > 0 => Some(Area { x, y, w, h }),
        _ => None,
    }
}

/// Finds the primary output's geometry, e.g. `DP-1 connected primary 3440x1440+0+0 ...`.
fn parse_primary(s: &str) -> Option<Area> {
    let line = s
        .lines()
        .find(|l| l.contains(" connected primary "))
        .or_else(|| s.lines().find(|l| l.contains(" connected ")))?;
    let geom = line
        .split_whitespace()
        .find(|w| w.contains('x') && w.contains('+'))?;
    let (size, pos) = geom.split_once('+')?;
    let (w, h) = size.split_once('x')?;
    let (x, y) = pos.split_once('+')?;
    Some(Area {
        x: x.parse().ok()?,
        y: y.parse().ok()?,
        w: w.parse().ok()?,
        h: h.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: i32, h: i32) -> Area {
        Area { x: 0, y: 0, w, h }
    }

    fn assert_9_16((w, h): (i32, i32)) {
        let ratio = w as f32 / h as f32;
        assert!((ratio - 9.0 / 16.0).abs() < 0.01, "{w}x{h}");
    }

    #[test]
    fn start_size_fits_work_area_and_keeps_ratio() {
        for (aw, ah) in [
            (3440, 1386),
            (1920, 1040),
            (1366, 728),
            (800, 600),
            (2160, 3840),
        ] {
            let (w, h) = initial_window_size(Some(area(aw, ah)));
            assert_9_16((w, h));
            assert!(h + DECORATION_H <= ah, "{w}x{h} too tall for {aw}x{ah}");
            assert!(w <= aw);
        }
        // A large screen gives a larger window than a small one.
        assert!(initial_window_size(Some(area(3440, 1386))).1 > 1200);
        assert_9_16(initial_window_size(None));
    }

    #[test]
    fn parses_xprop_and_xrandr() {
        assert_eq!(
            parse_workarea("_NET_WORKAREA(CARDINAL) = 0, 0, 3440, 1386\n"),
            Some(area(3440, 1386))
        );
        assert_eq!(parse_workarea("_NET_WORKAREA:  not found."), None);
        let xrandr = "Screen 0: minimum 16 x 16\n\
            HDMI-1 connected 1920x1080+3440+0 (normal) 500mm x 300mm\n\
            DP-1 connected primary 3440x1440+0+0 (normal left) 797mm x 334mm\n";
        assert_eq!(parse_primary(xrandr), Some(area(3440, 1440)));
    }

    #[test]
    fn multi_monitor_work_area_is_clipped_to_primary() {
        let work = Area {
            x: 0,
            y: 0,
            w: 5360,
            h: 1400,
        };
        let primary = Area {
            x: 3440,
            y: 0,
            w: 1920,
            h: 1080,
        };
        assert_eq!(
            work.intersect(primary),
            Some(Area {
                x: 3440,
                y: 0,
                w: 1920,
                h: 1080
            })
        );
    }
}
