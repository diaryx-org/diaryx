//! Centralized ARK blade minting, backed by v4-UUID entropy.
//!
//! Both entry `id`s (file blades) and workspace-registry IDs (workspace blades)
//! are minted here so the uuid-entropy plumbing — refilling a byte buffer from
//! `Uuid::new_v4()` as the [`diaryx_ark`] minter consumes it — lives in exactly
//! one place rather than being copy-pasted at each creation site.
//!
//! The whole module is gated on the `uuid` feature: builds without it (the dumb
//! server, isolated plugin builds) never mint IDs at runtime.

use std::collections::HashSet;

/// A byte source drawing entropy from v4 UUIDs, refilling a small buffer as the
/// ARK minter consumes it.
fn uuid_rng() -> impl FnMut() -> u8 {
    let mut buf: Vec<u8> = Vec::new();
    move || {
        if buf.is_empty() {
            buf.extend_from_slice(&uuid::Uuid::new_v4().into_bytes());
        }
        buf.pop().unwrap()
    }
}

/// Mint a unique ARK file blade (an entry `id`), retrying until the blade is
/// free of the `existing` set.
pub(crate) fn mint_file_blade(existing: &HashSet<String>) -> String {
    let mut rng = uuid_rng();
    diaryx_ark::mint_file_blade_unique(&mut rng, |b| existing.contains(b))
}

/// Mint a unique ARK workspace blade, retrying until `is_taken` reports the
/// blade is free.
pub(crate) fn mint_workspace_blade(is_taken: impl Fn(&str) -> bool) -> String {
    let mut rng = uuid_rng();
    diaryx_ark::mint_workspace_blade_unique(&mut rng, is_taken)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_valid_unique_file_blades() {
        let mut seen = HashSet::new();
        for _ in 0..50 {
            let blade = mint_file_blade(&seen);
            assert!(diaryx_ark::validate_file_blade(&blade).is_ok());
            assert!(seen.insert(blade), "blade should be unique");
        }
    }

    #[test]
    fn mints_valid_workspace_blade() {
        let blade = mint_workspace_blade(|_| false);
        assert!(diaryx_ark::validate_workspace_blade(&blade).is_ok());
    }
}
