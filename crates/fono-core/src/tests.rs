use super::*;

#[test]
fn logical_resources_change_atomically_with_the_authoritative_phase() {
    let coordinator = OperationCoordinator::new();
    let operation = coordinator.start(OperationSource::Ui).unwrap();
    coordinator
        .acquire_resource(operation.id, OperationResource::CommandProposal)
        .unwrap();
    for (phase, expected) in [
        (OperationPhase::Recording, Some(OperationResource::Audio)),
        (OperationPhase::Transcribing, Some(OperationResource::Stt)),
        (OperationPhase::Processing, None),
        (OperationPhase::AwaitingAction, None),
        (
            OperationPhase::Injecting,
            Some(OperationResource::Injection),
        ),
        (OperationPhase::AwaitingAction, None),
    ] {
        let event = coordinator.transition(operation.id, phase).unwrap();
        assert!(matches!(event, OperationEvent::PhaseChanged(snapshot) if snapshot.phase == phase));
        let mut resources = vec![OperationResource::CommandProposal];
        resources.extend(expected);
        resources.sort();
        assert_eq!(coordinator.resources(operation.id), Some(resources));
    }
    let before = coordinator.resources(operation.id);
    assert!(coordinator
        .transition(operation.id, OperationPhase::Recording)
        .is_none());
    assert_eq!(coordinator.resources(operation.id), before);
    coordinator
        .finish(operation.id, TerminalReason::Completed)
        .unwrap();
    assert_eq!(coordinator.resources(operation.id), None);
}

#[test]
fn awaiting_action_retains_operation_and_supports_processing_retry() {
    let coordinator = OperationCoordinator::new();
    let operation = coordinator.start(OperationSource::Hotkey).unwrap();
    coordinator
        .transition(operation.id, OperationPhase::Transcribing)
        .unwrap();
    coordinator
        .transition(operation.id, OperationPhase::AwaitingAction)
        .unwrap();
    assert!(coordinator.start(OperationSource::Ui).is_err());
    coordinator
        .transition(operation.id, OperationPhase::Processing)
        .unwrap();
    coordinator
        .transition(operation.id, OperationPhase::AwaitingAction)
        .unwrap();
    coordinator
        .transition(operation.id, OperationPhase::Injecting)
        .unwrap();
    coordinator
        .transition(operation.id, OperationPhase::AwaitingAction)
        .unwrap();
    coordinator.cancel(operation.id).unwrap();
    let next = coordinator.start(OperationSource::Ui).unwrap();
    assert!(coordinator
        .transition(operation.id, OperationPhase::Injecting)
        .is_none());
    assert_eq!(coordinator.current().unwrap().id, next.id);
}

#[test]
fn only_one_operation_owns_the_lease() {
    let coordinator = OperationCoordinator::new();
    let first = coordinator.start(OperationSource::Ui).unwrap();
    assert!(matches!(
        coordinator.start(OperationSource::Hotkey),
        Err(CoordinatorError::Busy(_))
    ));
    coordinator.finish(first.id, TerminalReason::Completed);
    assert!(coordinator.start(OperationSource::Hotkey).unwrap().id > first.id);
}

#[test]
fn service_requests_share_the_same_operation_lease() {
    let coordinator = OperationCoordinator::new();
    let service = coordinator.start(OperationSource::Service).unwrap();

    assert!(matches!(
        coordinator.start(OperationSource::Ui),
        Err(CoordinatorError::Busy(_))
    ));

    coordinator
        .finish(service.id, TerminalReason::Completed)
        .unwrap();
    assert!(coordinator.start(OperationSource::Ui).is_ok());
}

#[test]
fn legal_and_illegal_transitions_are_enforced() {
    let coordinator = OperationCoordinator::new();
    let operation = coordinator.start(OperationSource::Ui).unwrap();
    assert!(coordinator
        .transition(operation.id, OperationPhase::Processing)
        .is_none());
    assert!(coordinator
        .transition(operation.id, OperationPhase::Transcribing)
        .is_some());
    assert!(coordinator
        .transition(operation.id, OperationPhase::Processing)
        .is_some());
    assert!(coordinator
        .transition(operation.id, OperationPhase::Injecting)
        .is_some());
}

#[test]
fn stale_operation_cannot_mutate_the_current_one() {
    let coordinator = OperationCoordinator::new();
    let stale = coordinator.start(OperationSource::Ui).unwrap();
    coordinator.cancel(stale.id);
    let current = coordinator.start(OperationSource::WakeWord).unwrap();
    assert!(coordinator
        .transition(stale.id, OperationPhase::Transcribing)
        .is_none());
    assert_eq!(coordinator.current(), Some(current));
}

#[test]
fn cancellation_signal_survives_terminal_removal() {
    let coordinator = OperationCoordinator::new();
    let operation = coordinator.start(OperationSource::Ui).unwrap();
    let cancellation = coordinator.cancellation(operation.id).unwrap();
    coordinator.cancel(operation.id).unwrap();
    assert!(cancellation.is_cancelled());
}

#[test]
fn combined_cancellation_observes_either_owner() {
    let first = OperationCancellation::default();
    let second = OperationCancellation::default();
    let combined = first.combined_with(&second);
    assert!(!combined
        .atomic_signals()
        .any(|signal| signal.load(Ordering::Acquire)));

    second.cancel();

    assert!(combined.is_cancelled());
    assert!(combined
        .atomic_signals()
        .any(|signal| signal.load(Ordering::Acquire)));
}

#[test]
fn resource_leases_are_operation_scoped_and_cleared_by_terminal_failure() {
    let coordinator = OperationCoordinator::new();
    let operation = coordinator.start(OperationSource::Ui).unwrap();

    coordinator
        .acquire_resource(operation.id, OperationResource::Audio)
        .unwrap();
    assert!(matches!(
        coordinator.acquire_resource(operation.id, OperationResource::Audio),
        Err(CoordinatorError::ResourceAlreadyLeased { .. })
    ));
    assert_eq!(
        coordinator.resources(operation.id),
        Some(vec![OperationResource::Audio])
    );

    coordinator
        .finish(operation.id, TerminalReason::Failed)
        .unwrap();
    assert!(coordinator.resources(operation.id).is_none());
    assert!(!coordinator.release_resource(operation.id, OperationResource::Audio));
}

#[test]
fn one_thousand_terminal_cycles_leave_no_active_operation_or_resource_leases() {
    let coordinator = OperationCoordinator::new();

    for index in 0..1_000 {
        let operation = coordinator.start(OperationSource::Diagnostics).unwrap();
        coordinator
            .acquire_resource(operation.id, OperationResource::Audio)
            .unwrap();
        coordinator
            .acquire_resource(operation.id, OperationResource::Stt)
            .unwrap();
        coordinator
            .acquire_resource(operation.id, OperationResource::Injection)
            .unwrap();

        let terminal_event = if index % 2 == 0 {
            coordinator.cancel(operation.id).unwrap()
        } else {
            coordinator
                .finish(operation.id, TerminalReason::Failed)
                .unwrap()
        };

        assert!(matches!(
            terminal_event,
            OperationEvent::Finished(finished) if finished.id == operation.id
        ));
        assert!(coordinator.current().is_none());
        assert!(coordinator.resources(operation.id).is_none());
    }
}

#[test]
fn stale_operation_cannot_acquire_or_release_a_resource() {
    let coordinator = OperationCoordinator::new();
    let stale = coordinator.start(OperationSource::Ui).unwrap();
    coordinator.cancel(stale.id).unwrap();
    let current = coordinator.start(OperationSource::Hotkey).unwrap();

    assert_eq!(
        coordinator.acquire_resource(stale.id, OperationResource::Audio),
        Err(CoordinatorError::StaleOperation(stale.id))
    );
    assert!(!coordinator.release_resource(stale.id, OperationResource::Audio));
    assert_eq!(coordinator.resources(current.id), Some(Vec::new()));
}
