//! Only selected-button edges enter this state machine; no typing data is kept.
use super::state::KeyEvent;
use crate::types::{DictationTrigger, HotkeyMode};

const DOUBLE_PRESS_MS: u32 = 350;

#[derive(Debug, Clone, Copy)]
pub(super) struct Edge {
    pub pressed: bool,
    /// Windows input tick count, including its normal u32 wraparound.
    pub time: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Gesture {
    None,
    Edge(KeyEvent),
    Toggle,
}

pub(super) struct GestureState {
    double_press: bool,
    down: bool,
    pressed_at: Option<u32>,
    first_tap: Option<u32>,
}

impl GestureState {
    pub fn new(trigger: DictationTrigger, mode: HotkeyMode) -> Self {
        Self {
            double_press: trigger.is_keyboard() && mode == HotkeyMode::Toggle,
            down: false,
            pressed_at: None,
            first_tap: None,
        }
    }

    pub fn event(&mut self, event: Edge) -> Gesture {
        if event.pressed == self.down {
            return Gesture::None; // Autorepeat and duplicate/old releases.
        }
        self.down = event.pressed;
        if !self.double_press {
            return Gesture::Edge(if event.pressed {
                KeyEvent::Pressed
            } else {
                KeyEvent::Released
            });
        }
        if event.pressed {
            self.pressed_at = Some(event.time);
            if self
                .first_tap
                .take()
                .is_some_and(|first| event.time.wrapping_sub(first) <= DOUBLE_PRESS_MS)
            {
                self.pressed_at = None; // A third tap cannot reuse the second tap.
                return Gesture::Toggle;
            }
        } else if self
            .pressed_at
            .take()
            .is_some_and(|start| event.time.wrapping_sub(start) <= DOUBLE_PRESS_MS)
        {
            self.first_tap = Some(event.time);
        }
        Gesture::None
    }
}

#[cfg(test)]
mod tests;
