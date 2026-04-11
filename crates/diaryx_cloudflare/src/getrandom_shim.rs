//! Custom `getrandom` backend for the Cloudflare Worker.
//!
//! The workspace `.cargo/config.toml` sets
//! `--cfg getrandom_backend="custom"` for all `wasm32-unknown-unknown`
//! builds (so the extism plugin guests can supply their own PRNG). That
//! means this worker must also provide `__getrandom_v03_custom` — otherwise
//! the wasm module imports it as `env.__getrandom_v03_custom` and
//! worker-build's bundler stubs it with a function that writes no bytes,
//! making every `rand`/`uuid` call silently return zeros.
//!
//! We satisfy the extern in Rust by calling `globalThis.crypto.getRandomValues`
//! (available in Workers). Defining the symbol here removes the wasm import
//! entirely, so worker-build's stub becomes dead code and no JS patching is
//! required.

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

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_node_experimental);

    /// Verifies the shim actually fills the buffer with entropy from
    /// `crypto.getRandomValues`, not the all-zero stub worker-build emits
    /// when the symbol is unsatisfied.
    #[wasm_bindgen_test]
    fn getrandom_returns_real_entropy() {
        let mut buf = [0u8; 64];
        getrandom_03::fill(&mut buf).expect("getrandom should succeed");

        assert!(
            buf.iter().any(|&b| b != 0),
            "buffer is all zeros — broken stub is in effect"
        );

        // 64 truly random bytes essentially always have >20 distinct values.
        // (Birthday math: P(<=20 unique) is astronomically small.)
        let unique = buf.iter().collect::<std::collections::HashSet<_>>().len();
        assert!(
            unique > 20,
            "only {unique} unique bytes in 64 — entropy looks degenerate: {buf:?}"
        );

        // Two consecutive fills should differ.
        let mut buf2 = [0u8; 64];
        getrandom_03::fill(&mut buf2).expect("getrandom should succeed");
        assert_ne!(buf, buf2, "two fills returned identical bytes");
    }

    /// Exercises the >64 KiB chunking path.
    #[wasm_bindgen_test]
    fn getrandom_handles_large_buffers() {
        let mut buf = vec![0u8; 200_000];
        getrandom_03::fill(&mut buf).expect("getrandom should succeed for large buffer");
        let nonzero = buf.iter().filter(|&&b| b != 0).count();
        // ~99.6% of bytes should be nonzero on average.
        assert!(
            nonzero > 190_000,
            "large buffer entropy looks degenerate: {nonzero}/200000 nonzero"
        );
    }
}
