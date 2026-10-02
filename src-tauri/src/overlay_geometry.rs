//! Physical screen coordinates, including monitors left of the primary display.

#[derive(Clone, Copy)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn fits_screen(x: i32, y: i32, width: u32, height: u32, screen: ScreenRect) -> bool {
    // Windows uses -32000 for minimized windows. Never persist this sentinel.
    if x == -32000 || y == -32000 {
        return false;
    }
    let (x, y) = (i64::from(x), i64::from(y));
    let (left, top) = (i64::from(screen.x), i64::from(screen.y));
    x >= left
        && y >= top
        && x + i64::from(width) <= left + i64::from(screen.width)
        && y + i64::from(height) <= top + i64::from(screen.height)
}

pub fn center(width: u32, height: u32, screen: ScreenRect) -> (i32, i32) {
    (
        screen.x + (screen.width.saturating_sub(width) / 2) as i32,
        screen.y + (screen.height.saturating_sub(height) / 2) as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: ScreenRect = ScreenRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
    };

    #[test]
    fn rejects_minimized_and_disconnected_monitor_positions() {
        assert!(!fits_screen(-32000, -32000, 286, 88, PRIMARY));
        assert!(!fits_screen(2200, 40, 286, 88, PRIMARY));
        assert!(!fits_screen(1800, 40, 286, 88, PRIMARY));
        assert!(fits_screen(560, 40, 286, 88, PRIMARY));
    }

    #[test]
    fn keeps_valid_negative_coordinates_on_a_secondary_monitor() {
        let left = ScreenRect {
            x: -1920,
            ..PRIMARY
        };
        assert!(fits_screen(-1500, 40, 286, 88, left));
        assert_eq!(center(286, 88, left), (-1103, 476));
    }

    #[test]
    fn centers_scaled_overlay_inside_the_work_area() {
        assert_eq!(center(572, 176, PRIMARY), (674, 432));
        assert!(fits_screen(674, 432, 572, 176, PRIMARY));
    }
}
