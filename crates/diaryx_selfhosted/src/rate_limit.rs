//! Simple per-user sliding window rate limiter.

use dashmap::DashMap;
use std::time::{Duration, Instant};

/// A composite key: (user_id, endpoint_tag).
type Key = (String, &'static str);

/// Per-user sliding window rate limiter.
#[derive(Clone)]
pub struct RateLimiter {
    windows: DashMap<Key, Vec<Instant>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            windows: DashMap::new(),
        }
    }

    /// Check whether a request is allowed.
    ///
    /// Returns `Ok(())` if allowed, or `Err(retry_after_secs)` if the limit
    /// has been exceeded.
    pub fn check(
        &self,
        user_id: &str,
        endpoint: &'static str,
        limit: usize,
        window: Duration,
    ) -> Result<(), u64> {
        let key = (user_id.to_string(), endpoint);
        let now = Instant::now();
        let cutoff = now - window;

        let mut entry = self.windows.entry(key).or_default();
        let timestamps = entry.value_mut();

        // Remove expired timestamps
        timestamps.retain(|t| *t >= cutoff);

        if timestamps.len() >= limit {
            // Estimate retry-after from the oldest timestamp in the window
            let oldest = timestamps.first().copied().unwrap_or(now);
            let retry_after = (oldest + window).saturating_duration_since(now);
            return Err(retry_after.as_secs().max(1));
        }

        timestamps.push(now);
        Ok(())
    }

    /// Remove entries older than `max_age` to prevent unbounded growth.
    pub fn cleanup(&self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        self.windows.retain(|_, timestamps| {
            timestamps.retain(|t| *t >= cutoff);
            !timestamps.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimiter;
    use std::{thread::sleep, time::Duration};

    #[test]
    fn check_enforces_limit_and_recovers_after_window() {
        let limiter = RateLimiter::new();
        let window = Duration::from_millis(20);

        assert_eq!(limiter.check("user1", "proxy", 1, window), Ok(()));
        let retry_after = limiter
            .check("user1", "proxy", 1, window)
            .expect_err("should be rate limited");
        assert!(retry_after >= 1);

        sleep(window + Duration::from_millis(10));
        assert_eq!(limiter.check("user1", "proxy", 1, window), Ok(()));
    }

    #[test]
    fn cleanup_removes_expired_windows() {
        let limiter = RateLimiter::new();
        assert_eq!(
            limiter.check("user1", "namespaces", 2, Duration::from_secs(1)),
            Ok(())
        );
        assert_eq!(limiter.windows.len(), 1);

        sleep(Duration::from_millis(15));
        limiter.cleanup(Duration::from_millis(1));

        assert!(limiter.windows.is_empty());
    }
}
