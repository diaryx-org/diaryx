//! Helpers for mapping platform-specific error names to [`io::ErrorKind`].
//!
//! Browser filesystem APIs surface failures as `DOMException`s with a
//! short `name` string ("NotFoundError", "QuotaExceededError", ...). The
//! [`dom_exception_kind`] function maps those names onto the `std::io`
//! kinds so that backends like OPFS, IndexedDB, and the File System Access
//! API can produce errors that downstream code reacts to the same way it
//! does to native filesystem failures.
//!
//! `crossfs` itself does not depend on `wasm-bindgen` or `web-sys`; the
//! caller is expected to extract the `name` string and pass it here.

use std::io;

/// Map a `DOMException`'s `name` to a corresponding [`io::ErrorKind`].
///
/// Names not recognized fall through to [`io::ErrorKind::Other`]. The
/// matching is case-sensitive, matching the browser specification.
///
/// # Examples
///
/// ```
/// use crossfs::error::dom_exception_kind;
/// use std::io::ErrorKind;
///
/// assert_eq!(dom_exception_kind("NotFoundError"), ErrorKind::NotFound);
/// assert_eq!(dom_exception_kind("QuotaExceededError"), ErrorKind::QuotaExceeded);
/// assert_eq!(dom_exception_kind("BogusName"), ErrorKind::Other);
/// ```
pub fn dom_exception_kind(name: &str) -> io::ErrorKind {
    match name {
        "NotFoundError" => io::ErrorKind::NotFound,
        "TypeMismatchError" | "TypeError" => io::ErrorKind::InvalidInput,
        "InvalidStateError" => io::ErrorKind::InvalidInput,
        "InvalidModificationError" => io::ErrorKind::InvalidInput,
        "InvalidCharacterError" => io::ErrorKind::InvalidFilename,
        "QuotaExceededError" => io::ErrorKind::QuotaExceeded,
        "NotAllowedError" | "SecurityError" => io::ErrorKind::PermissionDenied,
        "NoModificationAllowedError" => io::ErrorKind::ReadOnlyFilesystem,
        "NotReadableError" => io::ErrorKind::PermissionDenied,
        "AbortError" => io::ErrorKind::Interrupted,
        _ => io::ErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_names_map_correctly() {
        assert_eq!(dom_exception_kind("NotFoundError"), io::ErrorKind::NotFound);
        assert_eq!(
            dom_exception_kind("QuotaExceededError"),
            io::ErrorKind::QuotaExceeded
        );
        assert_eq!(
            dom_exception_kind("NoModificationAllowedError"),
            io::ErrorKind::ReadOnlyFilesystem
        );
        assert_eq!(
            dom_exception_kind("NotAllowedError"),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn unknown_falls_through() {
        assert_eq!(dom_exception_kind("Mystery"), io::ErrorKind::Other);
        assert_eq!(dom_exception_kind(""), io::ErrorKind::Other);
    }
}
