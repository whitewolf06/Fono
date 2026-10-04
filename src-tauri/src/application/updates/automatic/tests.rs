use super::*;

#[test]
fn first_opt_in_is_due_but_restarting_does_not_bypass_daily_limit() {
    let now = 1_000_000;
    let mut schedule = Schedule::new(None, now);
    assert_eq!(schedule.delay(now), Duration::ZERO);
    schedule.record_attempt(now);
    assert_eq!(schedule.delay(now), INTERVAL);
    let restarted = Schedule::new(schedule.last_attempt, now + 60);
    assert_eq!(
        restarted.delay(now + 60),
        INTERVAL - Duration::from_secs(60)
    );
    assert_eq!(
        restarted.delay(now + INTERVAL.as_secs() - 1),
        Duration::from_secs(1)
    );
    assert_eq!(restarted.delay(now + INTERVAL.as_secs()), Duration::ZERO);
}

#[test]
fn failed_attempts_are_rate_limited_and_future_clock_is_bounded() {
    let now = 1_000_000;
    let schedule = Schedule::new(Some(now + 100 * INTERVAL.as_secs()), now);
    assert_eq!(schedule.delay(now), INTERVAL);
    assert_eq!(schedule.delay(now + INTERVAL.as_secs()), Duration::ZERO);
    let mut failed = Schedule::new(None, now);
    failed.record_attempt(now);
    assert_eq!(failed.delay(now + 1), INTERVAL - Duration::from_secs(1));
    assert_eq!(failed.delay(now - 60), INTERVAL);
}

#[tokio::test]
async fn opt_in_and_opt_out_wake_the_scheduler_without_waiting_a_day() {
    let changed = Notify::new();
    // Notify's permit also prevents a preference change between checking the
    // opt-in flag and starting to wait from being lost.
    changed.notify_one();
    tokio::time::timeout(Duration::from_secs(1), wait(&changed, INTERVAL))
        .await
        .unwrap();
    let waiting = wait(&changed, INTERVAL);
    tokio::pin!(waiting);
    assert!(tokio::time::timeout(Duration::from_millis(5), &mut waiting)
        .await
        .is_err());
    changed.notify_one();
    tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap();
}
