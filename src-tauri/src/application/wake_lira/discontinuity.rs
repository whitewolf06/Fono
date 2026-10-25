//! Discard stale acoustic context without cold-loading healthy models.
use std::time::Duration;

#[derive(Default)]
pub(super) struct RecoveryBudget {
    failures: std::collections::VecDeque<Duration>,
}
impl RecoveryBudget {
    pub fn allow(&mut self, now: Duration) -> bool {
        while self
            .failures
            .front()
            .is_some_and(|old| now.saturating_sub(*old) >= Duration::from_secs(30))
        {
            self.failures.pop_front();
        }
        if self.failures.len() >= 3 {
            return false;
        }
        self.failures.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_scheduling_gaps_rearm_but_sustained_degradation_is_terminal() {
        let mut budget = RecoveryBudget::default();
        for second in [1, 2, 3] {
            assert!(budget.allow(Duration::from_secs(second)));
        }
        assert!(!budget.allow(Duration::from_secs(4)));
        assert!(budget.allow(Duration::from_secs(31)));
        assert!(!budget.allow(Duration::from_secs(31)));
        assert!(budget.allow(Duration::from_secs(60)));
    }
}
