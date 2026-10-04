use super::*;

#[test]
fn stale_finish_does_not_take_the_successor_capture() {
    let mut slot = CaptureSlot {
        id: 12,
        owner: Some("new capture"),
    };
    assert!(slot.claim(11).is_err());
    assert_eq!(slot.owner, Some("new capture"));
    assert_eq!(slot.claim(12).unwrap(), "new capture");
}

#[test]
fn duplicate_finish_keeps_one_recognition_and_cleanup_owner() {
    let mut slot = CaptureSlot {
        id: 4,
        owner: Some(3),
    };
    assert_eq!(slot.claim(4).unwrap(), 3);
    assert!(matches!(slot.claim(4), Err(AppError::Busy(_))));
    assert!(slot.owner.is_none());
}
