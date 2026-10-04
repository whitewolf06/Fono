//! Pure key-edge state machine. A key release can stop only its own capture.
use crate::types::HotkeyMode;

#[derive(Debug, Clone, Copy)]
pub(super) enum KeyEvent {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ActiveCapture {
    pub operation: u64,
    pub recording: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    None,
    Start,
    Stop(u64),
}

#[derive(Default)]
pub(super) struct ShortcutState {
    key_down: bool,
    operation: Option<u64>,
}

impl ShortcutState {
    pub fn event(
        &mut self,
        event: KeyEvent,
        mode: HotkeyMode,
        active: Option<ActiveCapture>,
    ) -> Action {
        // Overlay Stop/Cancel, failed capture and completed processing invalidate
        // ownership without allowing an old release to affect a newer session.
        if self.operation.is_some()
            && !active.is_some_and(|capture| {
                Some(capture.operation) == self.operation && capture.recording
            })
        {
            self.operation = None;
        }
        match event {
            KeyEvent::Pressed => {
                if self.key_down {
                    return Action::None;
                }
                self.key_down = true;
                if mode == HotkeyMode::Toggle {
                    if let Some(operation) = self.operation.take() {
                        return Action::Stop(operation);
                    }
                }
                if active.is_none() {
                    Action::Start
                } else {
                    Action::None
                }
            }
            KeyEvent::Released => {
                if !self.key_down {
                    return Action::None;
                }
                self.key_down = false;
                if mode == HotkeyMode::Hold {
                    self.operation.take().map_or(Action::None, Action::Stop)
                } else {
                    Action::None
                }
            }
        }
    }

    pub fn started(&mut self, operation: u64) {
        self.operation = (operation != 0).then_some(operation);
    }

    pub fn restore_stop(&mut self, operation: u64, active: Option<ActiveCapture>) {
        if self.operation.is_none()
            && active.is_some_and(|capture| capture.operation == operation && capture.recording)
        {
            self.operation = Some(operation);
        }
    }
}

#[cfg(test)]
mod tests;
