//! Time helpers for sync metadata timestamps.
//!
//! Extism guest builds (`wasm32` without `browser-js`) cannot rely on
//! `std::time::SystemTime::now()`, which may panic at runtime.
//! Provide a monotonic fallback for that target while preserving wall-clock
//! timestamps everywhere else.

#[cfg(any(not(target_arch = "wasm32"), feature = "browser-js"))]
#[inline]
pub(crate) fn now_timestamp_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(all(target_arch = "wasm32", not(feature = "browser-js")))]
#[inline]
pub(crate) fn now_timestamp_millis() -> i64 {
    use core::sync::atomic::{AtomicI64, Ordering};

    const FALLBACK_EPOCH_MS: i64 = 1_700_000_000_000;
    static NEXT_TS: AtomicI64 = AtomicI64::new(FALLBACK_EPOCH_MS);

    NEXT_TS.fetch_add(1, Ordering::Relaxed)
}
