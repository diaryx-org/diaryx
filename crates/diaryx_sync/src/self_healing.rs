//! Self-healing health tracker for workspace validation.
//!
//! Tracks consecutive validation failures and recommends healing actions.
//! After a configurable number of consecutive failures, recommends a full
//! CRDT rebuild from the last known-good state (e.g., a git commit).

/// Action recommended by the health tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealingAction {
    /// Validation passed — proceed normally.
    Proceed,
    /// First failure(s) — skip the commit but don't rebuild yet.
    SkipCommit,
    /// Too many consecutive failures — rebuild CRDT from last known-good state.
    RebuildCrdt,
}

/// Tracks consecutive validation failures for a workspace.
///
/// # Usage
///
/// ```ignore
/// let mut tracker = HealthTracker::new();
///
/// // After each validation:
/// if report.is_ok() {
///     tracker.record_success();
///     // action is Proceed
/// } else {
///     let action = tracker.record_failure();
///     match action {
///         HealingAction::SkipCommit => { /* wait and retry */ }
///         HealingAction::RebuildCrdt => { /* trigger rebuild */ }
///         _ => {}
///     }
/// }
/// ```
pub struct HealthTracker {
    /// Number of consecutive validation failures.
    consecutive_failures: u32,
    /// Threshold at which to recommend a full rebuild.
    rebuild_threshold: u32,
}

impl HealthTracker {
    /// Create a new health tracker with default threshold (3 failures).
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            rebuild_threshold: 3,
        }
    }

    /// Create a new health tracker with a custom rebuild threshold.
    pub fn with_threshold(rebuild_threshold: u32) -> Self {
        Self {
            consecutive_failures: 0,
            rebuild_threshold,
        }
    }

    /// Record a successful validation. Resets the failure counter.
    pub fn record_success(&mut self) -> HealingAction {
        self.consecutive_failures = 0;
        HealingAction::Proceed
    }

    /// Record a validation failure. Returns the recommended action.
    pub fn record_failure(&mut self) -> HealingAction {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= self.rebuild_threshold {
            HealingAction::RebuildCrdt
        } else {
            HealingAction::SkipCommit
        }
    }

    /// Get the current consecutive failure count.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Returns true if the tracker has no recorded failures.
    pub fn is_healthy(&self) -> bool {
        self.consecutive_failures == 0
    }
}

impl Default for HealthTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker_is_healthy() {
        let tracker = HealthTracker::new();
        assert!(tracker.is_healthy());
        assert_eq!(tracker.consecutive_failures(), 0);
    }

    #[test]
    fn test_success_returns_proceed() {
        let mut tracker = HealthTracker::new();
        assert_eq!(tracker.record_success(), HealingAction::Proceed);
        assert!(tracker.is_healthy());
    }

    #[test]
    fn test_first_failure_returns_skip() {
        let mut tracker = HealthTracker::new();
        assert_eq!(tracker.record_failure(), HealingAction::SkipCommit);
        assert!(!tracker.is_healthy());
        assert_eq!(tracker.consecutive_failures(), 1);
    }

    #[test]
    fn test_three_failures_returns_rebuild() {
        let mut tracker = HealthTracker::new();
        assert_eq!(tracker.record_failure(), HealingAction::SkipCommit);
        assert_eq!(tracker.record_failure(), HealingAction::SkipCommit);
        assert_eq!(tracker.record_failure(), HealingAction::RebuildCrdt);
        assert_eq!(tracker.consecutive_failures(), 3);
    }

    #[test]
    fn test_success_resets_counter() {
        let mut tracker = HealthTracker::new();
        tracker.record_failure();
        tracker.record_failure();
        assert_eq!(tracker.consecutive_failures(), 2);

        tracker.record_success();
        assert!(tracker.is_healthy());
        assert_eq!(tracker.consecutive_failures(), 0);

        // After reset, need 3 more failures for rebuild
        assert_eq!(tracker.record_failure(), HealingAction::SkipCommit);
    }

    #[test]
    fn test_custom_threshold() {
        let mut tracker = HealthTracker::with_threshold(2);
        assert_eq!(tracker.record_failure(), HealingAction::SkipCommit);
        assert_eq!(tracker.record_failure(), HealingAction::RebuildCrdt);
    }

    #[test]
    fn test_continued_failures_stay_rebuild() {
        let mut tracker = HealthTracker::new();
        for _ in 0..3 {
            tracker.record_failure();
        }
        // Fourth failure should still return RebuildCrdt
        assert_eq!(tracker.record_failure(), HealingAction::RebuildCrdt);
        assert_eq!(tracker.consecutive_failures(), 4);
    }
}
