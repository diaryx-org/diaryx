//! Custom `getrandom` backend for the browser wasm module.
//!
//! The workspace `.cargo/config.toml` sets `--cfg getrandom_backend="custom"`
//! for all `wasm32-unknown-unknown` builds (so the extism plugin guests can
//! supply their own PRNG). That makes the cfg global, so every wasm artifact —
//! including this one — must provide `__getrandom_v03_custom`. We pull getrandom
//! 0.3 transitively through `uuid` (`v4` / `rng-getrandom`); without this symbol
//! the module fails to link (`undefined symbol: __getrandom_v03_custom`).
//!
//! We satisfy the extern by calling `globalThis.crypto.getRandomValues`, which
//! exists on both the window and Web Worker globals. Defining the symbol here
//! removes the wasm import entirely.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = crypto, js_name = getRandomValues, catch)]
    fn crypto_get_random_values(buf: &mut [u8]) -> Result<(), JsValue>;
}

// Web Crypto's getRandomValues refuses buffers larger than 64 KiB.
const MAX_CHUNK: usize = 65_536;

#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom_03::Error> {
    let buf = unsafe { core::slice::from_raw_parts_mut(dest, len) };
    for chunk in buf.chunks_mut(MAX_CHUNK) {
        crypto_get_random_values(chunk).map_err(|_| getrandom_03::Error::UNEXPECTED)?;
    }
    Ok(())
}
