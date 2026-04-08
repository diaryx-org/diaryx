//! Custom `getrandom` implementation for WASM plugins.
//!
//! WASM guests can't access browser crypto APIs directly. This module provides
//! a deterministic xorshift64-based PRNG seeded from the host's timestamp.
//!
//! # Usage
//!
//! Enable the `getrandom-shim` feature in your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! diaryx_plugin_sdk = { version = "0.1", features = ["getrandom-shim"] }
//! ```
//!
//! Then in your `lib.rs`, pull in the module so the symbols are linked:
//!
//! ```rust,ignore
//! // Activates the custom getrandom backends for WASM.
//! use diaryx_plugin_sdk::getrandom_shim as _;
//! ```
//!
//! If you also need getrandom 0.2 (via a patched crate), call
//! [`register_getrandom_v02`] from your crate root.
//!
//! # Security Note
//!
//! This is **not** cryptographically secure. It is suitable for generating
//! UUIDs and non-security-critical random values. Do not use for key
//! generation or other cryptographic purposes.

use std::sync::atomic::{AtomicU64, Ordering};

static RNG_STATE: AtomicU64 = AtomicU64::new(0);

fn get_seed() -> u64 {
    let ts = crate::host::time::timestamp_millis().unwrap_or(42);
    if ts == 0 { 42 } else { ts }
}

/// Fill a buffer with pseudo-random bytes using xorshift64.
pub fn xorshift_fill(buf: &mut [u8]) {
    let mut state = RNG_STATE.load(Ordering::Relaxed);
    if state == 0 {
        state = get_seed();
    }
    for byte in buf.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
    RNG_STATE.store(state, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// getrandom 0.3 backend
// ---------------------------------------------------------------------------

/// Custom backend for `getrandom` 0.3.
///
/// This is automatically linked when the `getrandom-shim` feature is enabled.
#[cfg(feature = "getrandom-shim")]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom_03::Error> {
    unsafe {
        let buf = core::slice::from_raw_parts_mut(dest, len);
        xorshift_fill(buf);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// getrandom 0.2 support
// ---------------------------------------------------------------------------

/// Register the custom backend for `getrandom` 0.2.
///
/// Call this macro in your crate root if you depend on `getrandom` 0.2
/// (typically via a patched crate):
///
/// ```rust,ignore
/// diaryx_plugin_sdk::register_getrandom_v02!();
/// ```
#[macro_export]
macro_rules! register_getrandom_v02 {
    () => {
        fn __diaryx_sdk_getrandom_v02(buf: &mut [u8]) -> Result<(), getrandom::Error> {
            $crate::getrandom_shim::xorshift_fill(buf);
            Ok(())
        }
        getrandom::register_custom_getrandom!(__diaryx_sdk_getrandom_v02);
    };
}
