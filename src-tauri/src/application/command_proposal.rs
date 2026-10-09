//! Command preview use case shared by command-entry sources.

use std::time::Duration;

use crate::state::AppState;
use crate::types::CommandProposal;
use fono_core::OperationSource;

const PROPOSAL_TTL: Duration = Duration::from_secs(30);

pub fn create(
    state: &AppState,
    operation_id: u64,
    source: OperationSource,
    original_text: String,
    confidence: Option<f32>,
) -> CommandProposal {
    let created_at = chrono::Utc::now();
    let settings_snapshot = state.command_settings_snapshot();
    let proposal = CommandProposal {
        id: state.next_command_proposal_id(),
        operation_id,
        source,
        normalized_action: crate::app_commands::normalize(&original_text),
        original_text,
        confidence,
        created_at,
        expires_at: created_at
            + chrono::Duration::from_std(PROPOSAL_TTL).expect("proposal TTL fits chrono"),
        settings_version: settings_snapshot.version,
        settings_snapshot,
    };
    state.set_pending_command_proposal(Some(proposal.clone()));
    proposal
}

#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use fono_core::OperationSource;

    use super::create;

    #[test]
    fn proposal_captures_normalized_action_and_settings_version() {
        let state = AppState::new();
        let proposal = create(
            &state,
            42,
            OperationSource::Hotkey,
            "  Громче!!! ".into(),
            None,
        );

        assert_eq!(proposal.operation_id, 42);
        assert_eq!(proposal.normalized_action, "громче");
        assert_eq!(
            proposal.settings_version,
            proposal.settings_snapshot.version
        );
        assert!(state.pending_command_proposal().is_some());
    }

    #[test]
    fn proposal_preserves_wake_word_source() {
        let state = AppState::new();
        let proposal = create(
            &state,
            43,
            OperationSource::WakeWord,
            "команда: открой telegram".into(),
            Some(0.91),
        );

        assert_eq!(proposal.source, OperationSource::WakeWord);
        assert_eq!(proposal.confidence, Some(0.91));
    }
}
