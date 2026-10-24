//! Resume bursts own one probe per window; stale callbacks cannot repair a
//! replacement or resurrect an interface after shutdown.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Surface {
    Settings,
    Overlay,
}

impl Surface {
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "settings" => Some(Self::Settings),
            "overlay" => Some(Self::Overlay),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Overlay => "overlay",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Ticket {
    pub surface: Surface,
    pub instance: u64,
    sequence: u64,
}

#[derive(Default)]
struct Slot {
    instance: u64,
    pending: Option<Ticket>,
}

#[derive(Default)]
pub(super) struct Policy {
    next: u64,
    slots: [Slot; 2],
    closed: bool,
}

impl Policy {
    fn slot(&self, surface: Surface) -> &Slot {
        &self.slots[surface as usize]
    }

    fn slot_mut(&mut self, surface: Surface) -> &mut Slot {
        &mut self.slots[surface as usize]
    }

    pub fn attach(&mut self, surface: Surface) -> u64 {
        self.next += 1;
        let instance = self.next;
        *self.slot_mut(surface) = Slot {
            instance,
            pending: None,
        };
        instance
    }

    pub fn request(&mut self, surface: Surface, instance: u64) -> Option<Ticket> {
        let slot = self.slot(surface);
        if self.closed || slot.instance != instance || slot.pending.is_some() {
            return None;
        }
        self.next += 1;
        let ticket = Ticket {
            surface,
            instance,
            sequence: self.next,
        };
        self.slot_mut(surface).pending = Some(ticket);
        Some(ticket)
    }

    pub fn current(&self, ticket: Ticket) -> bool {
        !self.closed && self.slot(ticket.surface).pending == Some(ticket)
    }

    pub fn finish(&mut self, ticket: Ticket) {
        if self.current(ticket) {
            self.slot_mut(ticket.surface).pending = None;
        }
    }

    pub fn detach(&mut self, surface: Surface, instance: u64) {
        if self.slot(surface).instance == instance {
            *self.slot_mut(surface) = Slot::default();
        }
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.slots = Default::default();
    }

    pub fn restore_allowed(&self, ticket: Ticket) -> bool {
        let instance = self.slot(ticket.surface).instance;
        !self.closed && (instance == 0 || instance == ticket.instance)
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

#[derive(Default)]
pub(super) struct InFlight(std::sync::atomic::AtomicUsize);

pub(super) struct FlightGuard<'a>(&'a InFlight);

impl InFlight {
    pub fn start(&self) -> FlightGuard<'_> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        FlightGuard(self)
    }
    pub fn active(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst) > 0
    }
}
impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        self.0 .0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// This port cannot navigate, replace a document, focus or show a native window.
pub(super) trait RenderSurface {
    type Error;
    fn visible(&self) -> Result<bool, Self::Error>;
    fn set_visible(&self, visible: bool) -> Result<(), Self::Error>;
    fn refresh_bounds(&self) -> Result<(), Self::Error>;
}

pub(super) fn refresh<S: RenderSurface>(surface: &S) -> Result<(), S::Error> {
    if !surface.visible()? {
        return surface.refresh_bounds();
    }
    surface.set_visible(false)?;
    let refresh = surface.refresh_bounds();
    // Restore even after a bounds error: do not leave a healthy controller hidden.
    let restore = surface.set_visible(true);
    refresh.and(restore)
}

#[cfg(test)]
mod tests;
