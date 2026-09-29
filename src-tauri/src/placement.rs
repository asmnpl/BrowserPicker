//! Where the picker goes on screen. Works in a y-down space in a single unit
//! (physical pixels on Windows, points on macOS after flipping Cocoa's y axis).

use crate::config::Placement;

/// Keeps the picker this far from the edges of the usable screen area.
pub const MARGIN: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    /// Used to find the display under the cursor on macOS.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// Top-left corner for a `width`×`height` window inside `area` (the monitor's work area).
pub fn position(
    area: Rect,
    cursor: (f64, f64),
    width: f64,
    height: f64,
    placement: Placement,
    margin: f64,
) -> (f64, f64) {
    let (x, y) = match placement {
        // Center on the cursor horizontally, with the cursor over the browser row.
        Placement::Cursor => (cursor.0 - width / 2.0, cursor.1 - height * 0.4),
        Placement::Center => (area.x + (area.width - width) / 2.0, area.y + (area.height - height) * 0.4),
    };
    let x = x.min(area.x + area.width - width - margin).max(area.x + margin);
    let y = y.min(area.y + area.height - height - margin).max(area.y + margin);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = Rect { x: 0.0, y: 0.0, width: 1440.0, height: 900.0 };
    // A second display to the right of the primary one, taller and offset upwards.
    const SECONDARY: Rect = Rect { x: 1440.0, y: -200.0, width: 1920.0, height: 1080.0 };

    #[test]
    fn follows_the_cursor_on_any_display() {
        assert_eq!(position(PRIMARY, (700.0, 400.0), 400.0, 100.0, Placement::Cursor, 8.0), (500.0, 360.0));
        assert_eq!(
            position(SECONDARY, (2400.0, 300.0), 400.0, 100.0, Placement::Cursor, 8.0),
            (2200.0, 260.0)
        );
    }

    #[test]
    fn stays_inside_the_work_area() {
        assert_eq!(
            position(SECONDARY, (1445.0, -195.0), 400.0, 100.0, Placement::Cursor, 8.0),
            (1448.0, -192.0)
        );
        assert_eq!(position(PRIMARY, (1439.0, 899.0), 400.0, 100.0, Placement::Cursor, 8.0), (1032.0, 792.0));
    }

    #[test]
    fn centers_on_the_display() {
        assert_eq!(position(SECONDARY, (0.0, 0.0), 400.0, 100.0, Placement::Center, 8.0), (2200.0, 192.0));
    }

    #[test]
    fn finds_the_display_under_the_cursor() {
        assert!(SECONDARY.contains(2000.0, -100.0));
        assert!(!PRIMARY.contains(2000.0, -100.0));
        assert!(PRIMARY.contains(1440.0, 900.0));
    }
}
