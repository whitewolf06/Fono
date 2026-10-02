//! Application-level use cases behind the Tauri IPC facade.

pub mod audio_ingest;
pub mod command_proposal;
pub mod desktop_transcription_runtime;
pub mod diagnostics;
pub mod dictation;
pub mod dictation_tail_diagnostics;
pub mod local_transcription_service;
pub mod models;
pub mod rest_api;
pub mod service_control;
pub mod speech_analysis_queue;
pub mod transcription_contract;
pub mod transcription_jobs;
pub mod transcription_service;
pub mod wake;
pub mod wake_calibration;
pub mod wake_validation;
