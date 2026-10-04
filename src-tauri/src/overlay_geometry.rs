//! Stable overlay anchors and physical monitor bounds, independent of window APIs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Toolbar,
    QuickProcessing,
    Pending,
}

pub fn display_mode(
    pending: bool,
    listening: bool,
    preview: bool,
    quick_enabled: bool,
) -> DisplayMode {
    if pending {
        DisplayMode::Pending
    } else if (listening || preview) && quick_enabled {
        DisplayMode::QuickProcessing
    } else {
        DisplayMode::Toolbar
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Default)]
pub struct GeometryState {
    pub help_open: bool,
    pub error_visible: bool,
    pub anchor: Option<(i32, i32)>,
    applied_position: Option<(i32, i32)>,
    unsaved_drag_anchor: Option<(i32, i32)>,
    programmatic_position: Option<(i32, i32)>,
}

impl GeometryState {
    pub fn set_anchor(&mut self, position: (i32, i32)) {
        self.anchor = Some(position);
        self.applied_position = None;
        self.unsaved_drag_anchor = None;
    }

    pub fn mark_programmatic_move(&mut self, position: (i32, i32)) {
        self.programmatic_position = Some(position);
        self.remember_layout(position);
    }

    pub fn remember_layout(&mut self, position: (i32, i32)) {
        self.applied_position = Some(position);
    }

    /// A native drag can precede the renderer's debounced save by 500ms. Keep
    /// its current position before resizing, without persisting a clamp offset.
    pub fn observe_position(&mut self, position: (i32, i32)) {
        if position.0 == -32000 || position.1 == -32000 {
            return;
        }
        if self
            .applied_position
            .is_some_and(|applied| applied != position)
        {
            self.anchor = Some(position);
            self.unsaved_drag_anchor = Some(position);
            self.programmatic_position = None;
        }
    }

    /// Resize movements cannot replace the user anchor. If resizing interrupted
    /// a real drag's debounce, persist that original drag once instead of losing it.
    pub fn position_to_save(&mut self, position: (i32, i32)) -> Option<(i32, i32)> {
        if self.programmatic_position == Some(position) {
            return self.unsaved_drag_anchor.take();
        }
        self.programmatic_position = None;
        self.unsaved_drag_anchor = None;
        self.anchor = Some(position);
        self.applied_position = Some(position);
        Some(position)
    }
}

pub fn logical_size(
    mode: DisplayMode,
    compact: bool,
    help_open: bool,
    error_visible: bool,
    scale: f32,
) -> (f64, f64) {
    let (width, mut height) = match (mode, compact) {
        (DisplayMode::Toolbar, false) => (360.0, 104.0),
        (DisplayMode::Toolbar, true) => (320.0, 72.0),
        (DisplayMode::QuickProcessing, false) => (440.0, 192.0),
        (DisplayMode::QuickProcessing, true) => (400.0, 168.0),
        (DisplayMode::Pending, false) => (440.0, 292.0),
        (DisplayMode::Pending, true) => (400.0, 272.0),
    };
    if help_open {
        height += 120.0;
    }
    if error_visible && mode == DisplayMode::Toolbar {
        height += 48.0;
    }
    let scale = f64::from(if scale.is_finite() {
        scale.clamp(0.5, 2.0)
    } else {
        1.0
    });
    (width * scale, height * scale)
}

pub fn fit_at_anchor(
    anchor: (i32, i32),
    logical: (f64, f64),
    monitor_scale: f64,
    screen: ScreenRect,
) -> WindowRect {
    let dpi = valid_dpi(monitor_scale);
    let width = physical_length(logical.0, dpi, screen.width);
    let height = physical_length(logical.1, dpi, screen.height);
    let x = i64::from(anchor.0).clamp(
        i64::from(screen.x),
        i64::from(screen.x) + i64::from(screen.width.saturating_sub(width)),
    );
    let y = i64::from(anchor.1).clamp(
        i64::from(screen.y),
        i64::from(screen.y) + i64::from(screen.height.saturating_sub(height)),
    );
    WindowRect {
        x: clamp_coordinate(x),
        y: clamp_coordinate(y),
        width,
        height,
    }
}

pub fn valid_dpi(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn physical_length(logical: f64, dpi: f64, bound: u32) -> u32 {
    ((logical * dpi).round() as u32).clamp(1, bound.max(1))
}

fn clamp_coordinate(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
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
        clamp_coordinate(i64::from(screen.x) + i64::from(screen.width.saturating_sub(width) / 2)),
        clamp_coordinate(i64::from(screen.y) + i64::from(screen.height.saturating_sub(height) / 2)),
    )
}

#[cfg(test)]
#[path = "overlay_geometry_tests.rs"]
mod tests;
