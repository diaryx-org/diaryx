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
