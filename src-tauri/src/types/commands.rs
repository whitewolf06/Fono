//! Confirmable voice command contracts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchApp {
    pub name: String,
    pub exe_path: String,
    pub aliases: Vec<String>,
}

/// Immutable settings required to execute a confirmed voice-command proposal.
/// Secrets and unrelated UI settings are deliberately excluded.
#[derive(Debug, Clone)]
pub struct CommandSettingsSnapshot {
    pub version: u64,
    pub launch_apps: Vec<LaunchApp>,
    pub volume_step: u32,
}

/// A command is always previewed before it may affect another application.
/// The payload is serialisable so the renderer can show the proposal without
/// reaching into application state.
#[derive(Debug, Clone, Serialize)]
pub struct CommandProposal {
    pub id: u64,
    pub operation_id: u64,
    pub source: fono_core::OperationSource,
    pub original_text: String,
    pub normalized_action: String,
    pub confidence: Option<f32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub settings_version: u64,
    #[serde(skip)]
    pub settings_snapshot: CommandSettingsSnapshot,
}

impl CommandProposal {
    pub fn is_expired_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        now >= self.expires_at
    }
}
