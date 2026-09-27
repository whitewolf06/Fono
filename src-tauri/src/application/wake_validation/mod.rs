//! Fresh, aggregate-only validation before a calibrated Ramzi profile may
//! activate the live Sherpa listener.

mod rules;
mod service;
mod state;

pub use rules::{
    WakeProfileValidationInputIssue, WakeProfileValidationKind, WakeProfileValidationSampleResult,
    WakeProfileValidationStatus,
};
pub use service::{ensure_profile_can_activate, record, start, status};
pub use state::WakeProfileValidationService;
