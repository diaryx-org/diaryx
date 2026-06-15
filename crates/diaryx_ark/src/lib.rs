//! `diaryx_ark` — opaque ARK-style identifier minting and validation.
//!
//! ARK form: `ark:99999/dx<6 betanum><check>/<5 betanum><check>`
//!
//! - `99999` — NAAN placeholder (until Diaryx registers a real one).
//! - `dx` — the "shoulder", reserved so the ID format can change later.
//! - workspace blade: `dx` + 6 random betanumeric chars + 1 check char (9 total).
//! - file blade: 5 random betanumeric chars + 1 check char (6 total).
//!
//! Minting is **random** (opaque for free — no sequence to hide), with
//! uniqueness enforced by the caller via rejection (`*_unique`). Entropy is
//! caller-supplied so this crate stays dependency-free and target-agnostic.
//!
//! The check character is NOID-style: `sum(ordinal * 1-based-position) mod 28`,
//! mapped back into the alphabet. For the workspace blade the check covers the
//! whole blade *including* the `dx` shoulder. Characters outside the alphabet
//! contribute ordinal 0 but still advance the position counter.

use std::error::Error;
use std::fmt;

/// The 28-character betanumeric alphabet: no vowels (avoids accidental words),
/// no `0`/`1`/`l` (ambiguous), includes `y`.
pub const ALPHABET: &[u8; 28] = b"bcdfghjkmnpqrstvwxyz23456789";

/// Number of symbols in [`ALPHABET`] — the radix for the check char.
pub const RADIX: usize = ALPHABET.len();

/// NAAN placeholder until a real Name Assigning Authority Number is registered.
pub const NAAN: &str = "99999";

/// The shoulder prefix on every workspace blade.
pub const SHOULDER: &str = "dx";

/// Count of random characters in a workspace blade (excludes shoulder + check).
pub const WORKSPACE_RANDOM_LEN: usize = 6;
/// Count of random characters in a file blade (excludes check).
pub const FILE_RANDOM_LEN: usize = 5;

/// Total length of a workspace blade (`dx` + 6 random + 1 check).
pub const WORKSPACE_BLADE_LEN: usize = SHOULDER.len() + WORKSPACE_RANDOM_LEN + 1;
/// Total length of a file blade (5 random + 1 check).
pub const FILE_BLADE_LEN: usize = FILE_RANDOM_LEN + 1;

/// Errors produced while validating or parsing ARK components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArkError {
    /// A blade was not the expected length.
    BadLength,
    /// A character was outside [`ALPHABET`] (or the shoulder was wrong).
    BadChar,
    /// The trailing check character did not match the computed value.
    BadCheck,
    /// The overall `ark:` string was not shaped as expected.
    BadFormat,
}

impl fmt::Display for ArkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            ArkError::BadLength => "ARK component has an invalid length",
            ArkError::BadChar => "ARK component contains an invalid character",
            ArkError::BadCheck => "ARK check character does not match",
            ArkError::BadFormat => "ARK string is malformed",
        };
        f.write_str(msg)
    }
}

impl Error for ArkError {}

/// Returns the 0-based ordinal of `c` in [`ALPHABET`], or `None` if absent.
#[inline]
fn ordinal(c: char) -> Option<usize> {
    if c.is_ascii() {
        ALPHABET.iter().position(|&b| b as char == c)
    } else {
        None
    }
}

/// Computes the NOID-style check character over `blade_without_check`.
///
/// Each character contributes `ordinal * (1-based position)`; characters not in
/// [`ALPHABET`] contribute 0 but still advance the position. The sum modulo
/// [`RADIX`] indexes the check character.
pub fn check_char(blade_without_check: &str) -> char {
    let mut sum: usize = 0;
    for (i, c) in blade_without_check.chars().enumerate() {
        let ord = ordinal(c).unwrap_or(0);
        sum += ord * (i + 1);
    }
    ALPHABET[sum % RADIX] as char
}

/// Returns `true` if `blade_with_check`'s trailing character is the correct
/// check character for the preceding body. Returns `false` for an empty string.
pub fn verify_check(blade_with_check: &str) -> bool {
    let Some((last_idx, last)) = blade_with_check.char_indices().next_back() else {
        return false;
    };
    let body = &blade_with_check[..last_idx];
    check_char(body) == last
}

/// Pulls the next alphabet character from `rng`, rejecting bytes that would
/// bias the modulo (anything `>= RADIX * (256 / RADIX)`), so the output stays
/// uniform — i.e. genuinely opaque.
fn next_char(rng: &mut impl FnMut() -> u8) -> char {
    // 256 is not a multiple of 28; the largest usable multiple is 28*9 = 252.
    const LIMIT: u8 = (256 / RADIX as u16 * RADIX as u16) as u8; // 252
    loop {
        let b = rng();
        if b < LIMIT {
            return ALPHABET[(b as usize) % RADIX] as char;
        }
    }
}

fn random_body(rng: &mut impl FnMut() -> u8, len: usize) -> String {
    (0..len).map(|_| next_char(rng)).collect()
}

/// Mints a file blade: 5 random characters plus a check character.
pub fn mint_file_blade(rng: &mut impl FnMut() -> u8) -> String {
    let mut blade = random_body(rng, FILE_RANDOM_LEN);
    blade.push(check_char(&blade));
    blade
}

/// Mints a workspace blade: the `dx` shoulder, 6 random characters, and a check
/// character computed over the shoulder + random body.
pub fn mint_workspace_blade(rng: &mut impl FnMut() -> u8) -> String {
    let mut blade = String::with_capacity(WORKSPACE_BLADE_LEN);
    blade.push_str(SHOULDER);
    blade.push_str(&random_body(rng, WORKSPACE_RANDOM_LEN));
    blade.push(check_char(&blade));
    blade
}

/// Mints a file blade, retrying until `is_taken` reports the blade is free.
///
/// The 17M-element file namespace makes exhaustion a non-concern in practice;
/// the caller owns the taken-set (e.g. a workspace's existing `id`s).
pub fn mint_file_blade_unique(
    rng: &mut impl FnMut() -> u8,
    is_taken: impl Fn(&str) -> bool,
) -> String {
    loop {
        let blade = mint_file_blade(rng);
        if !is_taken(&blade) {
            return blade;
        }
    }
}

/// Mints a workspace blade, retrying until `is_taken` reports the blade is free.
pub fn mint_workspace_blade_unique(
    rng: &mut impl FnMut() -> u8,
    is_taken: impl Fn(&str) -> bool,
) -> String {
    loop {
        let blade = mint_workspace_blade(rng);
        if !is_taken(&blade) {
            return blade;
        }
    }
}

fn all_in_alphabet(s: &str) -> bool {
    s.chars().all(|c| ordinal(c).is_some())
}

/// Validates a file blade: 6 characters, all in [`ALPHABET`], correct check.
pub fn validate_file_blade(s: &str) -> Result<(), ArkError> {
    if s.chars().count() != FILE_BLADE_LEN {
        return Err(ArkError::BadLength);
    }
    if !all_in_alphabet(s) {
        return Err(ArkError::BadChar);
    }
    if !verify_check(s) {
        return Err(ArkError::BadCheck);
    }
    Ok(())
}

/// Validates a workspace blade: `dx` shoulder, 9 characters total, all in
/// [`ALPHABET`], correct check (which covers the shoulder).
pub fn validate_workspace_blade(s: &str) -> Result<(), ArkError> {
    if s.chars().count() != WORKSPACE_BLADE_LEN {
        return Err(ArkError::BadLength);
    }
    if !s.starts_with(SHOULDER) {
        return Err(ArkError::BadFormat);
    }
    if !all_in_alphabet(s) {
        return Err(ArkError::BadChar);
    }
    if !verify_check(s) {
        return Err(ArkError::BadCheck);
    }
    Ok(())
}

/// A parsed ARK, borrowing from the source string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ark<'a> {
    pub naan: &'a str,
    pub workspace_blade: &'a str,
    pub file_blade: &'a str,
}

/// Composes a full ARK string from validated-or-unvalidated blades.
pub fn format_ark(workspace_blade: &str, file_blade: &str) -> String {
    format!("ark:{NAAN}/{workspace_blade}/{file_blade}")
}

/// Parses a core ARK string (`ark:<naan>/<workspace>/<file>`).
///
/// Layer 1 parses only the three core segments and validates both blades;
/// `.FILE` / `?query` / `#callout` suffixes are a later layer and are rejected
/// here as [`ArkError::BadFormat`].
pub fn parse_ark(s: &str) -> Result<Ark<'_>, ArkError> {
    let rest = s.strip_prefix("ark:").ok_or(ArkError::BadFormat)?;
    let mut parts = rest.split('/');
    let naan = parts.next().ok_or(ArkError::BadFormat)?;
    let workspace_blade = parts.next().ok_or(ArkError::BadFormat)?;
    let file_blade = parts.next().ok_or(ArkError::BadFormat)?;
    if parts.next().is_some() {
        return Err(ArkError::BadFormat);
    }
    if naan.is_empty() {
        return Err(ArkError::BadFormat);
    }
    validate_workspace_blade(workspace_blade)?;
    validate_file_blade(file_blade)?;
    Ok(Ark {
        naan,
        workspace_blade,
        file_blade,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic byte source for tests: cycles through a fixed sequence.
    fn seq_rng(bytes: Vec<u8>) -> impl FnMut() -> u8 {
        let mut i = 0;
        move || {
            let b = bytes[i % bytes.len()];
            i += 1;
            b
        }
    }

    #[test]
    fn check_char_known_vectors() {
        // Hand-computed: b c d f g -> ordinals 0 1 2 3 4 at positions 1..=5
        // 0*1 + 1*2 + 2*3 + 3*4 + 4*5 = 40 ; 40 % 28 = 12 -> ALPHABET[12] = 'r'
        assert_eq!(check_char("bcdfg"), 'r');

        // Workspace body "dxbcdfgh": d x b c d f g h
        // 2*1 + 17*2 + 0*3 + 1*4 + 2*5 + 3*6 + 4*7 + 5*8 = 136 ; 136 % 28 = 24 -> '6'
        assert_eq!(check_char("dxbcdfgh"), '6');
    }

    #[test]
    fn check_char_treats_unknown_as_zero_but_advances_position() {
        // "b-d": b(0)*1 + '-'(0)*2 + d(2)*3 = 6 ; 6 % 28 = 6 -> ALPHABET[6] = 'j'
        assert_eq!(check_char("b-d"), 'j');
    }

    #[test]
    fn verify_check_round_trips_minted_blades() {
        let mut rng = seq_rng((0u8..=251).collect());
        for _ in 0..1000 {
            let f = mint_file_blade(&mut rng);
            assert!(verify_check(&f), "file blade {f} failed verify");
            assert!(validate_file_blade(&f).is_ok(), "file blade {f} invalid");

            let w = mint_workspace_blade(&mut rng);
            assert!(verify_check(&w), "workspace blade {w} failed verify");
            assert!(
                validate_workspace_blade(&w).is_ok(),
                "workspace blade {w} invalid"
            );
        }
    }

    #[test]
    fn verify_check_rejects_empty() {
        assert!(!verify_check(""));
    }

    #[test]
    fn minted_blades_have_expected_lengths() {
        let mut rng = seq_rng(vec![0, 30, 60, 90, 120, 150, 180, 210, 240]);
        assert_eq!(mint_file_blade(&mut rng).chars().count(), FILE_BLADE_LEN);
        assert_eq!(
            mint_workspace_blade(&mut rng).chars().count(),
            WORKSPACE_BLADE_LEN
        );
    }

    #[test]
    fn next_char_rejects_biasing_bytes() {
        // 252..=255 must be skipped; the next usable byte is 0 -> 'b'.
        let mut rng = seq_rng(vec![252, 253, 254, 255, 0]);
        assert_eq!(next_char(&mut rng), 'b');
    }

    #[test]
    fn workspace_blade_check_covers_shoulder() {
        let mut rng = seq_rng((0u8..=251).collect());
        let blade = mint_workspace_blade(&mut rng);
        assert!(validate_workspace_blade(&blade).is_ok());

        // Corrupt the shoulder: the check char was computed over the shoulder,
        // so swapping its first char must now fail the check (not the format,
        // since "bx" is still all-alphabet but isn't the "dx" shoulder).
        assert!(blade.starts_with("dx"));
        let corrupted = format!("b{}", &blade[1..]);
        // "bx..." fails the shoulder check before we even reach the check char.
        assert_eq!(
            validate_workspace_blade(&corrupted),
            Err(ArkError::BadFormat),
            "corrupting the shoulder should be rejected"
        );

        // Now corrupt a shoulder char in a way that keeps a valid-looking
        // prefix but proves the check covers the shoulder: rebuild the blade
        // with the shoulder's check contribution changed. Append the body's
        // own check over a shoulder-less body and confirm it differs.
        let body_without_shoulder = &blade[SHOULDER.len()..blade.len() - 1];
        assert_ne!(
            check_char(&blade[..blade.len() - 1]),
            check_char(body_without_shoulder),
            "check char must depend on the shoulder"
        );
    }

    #[test]
    fn validation_rejects_bad_inputs() {
        assert_eq!(validate_file_blade("bcdf"), Err(ArkError::BadLength));
        assert_eq!(validate_file_blade("bcdfga"), Err(ArkError::BadChar)); // 'a' not in alphabet
        // Wrong check char (body "bcdfg" should check to 'r', not 'b'):
        assert_eq!(validate_file_blade("bcdfgb"), Err(ArkError::BadCheck));

        assert_eq!(
            validate_workspace_blade("dxbcdfg"),
            Err(ArkError::BadLength)
        );
        assert_eq!(
            validate_workspace_blade("zzbcdfgh6"),
            Err(ArkError::BadFormat)
        ); // bad shoulder
    }

    #[test]
    fn parse_and_format_round_trip() {
        let mut rng = seq_rng((0u8..=251).collect());
        let ws = mint_workspace_blade(&mut rng);
        let file = mint_file_blade(&mut rng);
        let s = format_ark(&ws, &file);
        assert!(s.starts_with("ark:99999/"));

        let parsed = parse_ark(&s).expect("should parse");
        assert_eq!(parsed.naan, NAAN);
        assert_eq!(parsed.workspace_blade, ws);
        assert_eq!(parsed.file_blade, file);
    }

    #[test]
    fn parse_rejects_malformed() {
        assert_eq!(parse_ark("notanark"), Err(ArkError::BadFormat));
        assert_eq!(parse_ark("ark:99999/dxbcdfgh6"), Err(ArkError::BadFormat)); // missing file
        // Extra segment / suffix not supported in Layer 1:
        assert_eq!(
            parse_ark("ark:99999/dxbcdfgh6/bcdfgr/extra"),
            Err(ArkError::BadFormat)
        );
    }

    #[test]
    fn mint_unique_avoids_taken_blades() {
        // Force a collision on the first mint, then accept the second.
        let mut rng = seq_rng((0u8..=251).collect());
        let first = {
            let mut probe = seq_rng((0u8..=251).collect());
            mint_file_blade(&mut probe)
        };
        let taken = move |b: &str| b == first;
        let result = mint_file_blade_unique(&mut rng, taken);
        // The unique mint must not return the (taken) first blade.
        let mut probe = seq_rng((0u8..=251).collect());
        let first_again = mint_file_blade(&mut probe);
        assert_ne!(result, first_again);
        assert!(validate_file_blade(&result).is_ok());
    }
}
