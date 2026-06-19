//! Bridge between Tauri's serde-based IPC and `diaryx_core`'s fig-based types.
//!
//! Several `diaryx_core` types crossing the JS↔Rust IPC boundary were migrated
//! from serde to fig's `ToValue`/`FromValue` traits and no longer implement
//! serde's `Serialize`/`Deserialize`. Tauri (de)serializes command args and
//! return values with serde, so at the boundary we traffic in
//! `serde_json::Value` (which IS serde-(de)serializable and which Tauri maps
//! to/from normal JS objects) and bridge to/from the fig-only core types here.
//!
//! This keeps the JS-visible shape identical (frontend untouched) while leaving
//! the core types serde-free.

/// Convert a core fig value into a `serde_json::Value` for returning to JS.
///
/// Goes through fig's JSON string serializer (in this fig rev `fig::Value`
/// implements serde `Deserialize` but not `Serialize`), then parses that into a
/// `serde_json::Value`. Returns `Value::Null` on failure.
pub fn fig_to_json<T: diaryx_core::fig::ToValue + ?Sized>(value: &T) -> serde_json::Value {
    diaryx_core::fig::ToValue::to_value(value)
        .serialize_with(
            diaryx_core::fig::Format::Json,
            diaryx_core::fig::SerializeOptions::compact(),
        )
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_end()).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Tauri-side, serde-serializable mirror of [`diaryx_core::error::SerializableError`].
///
/// `diaryx_core`'s `SerializableError` was migrated to fig (`fig::ToValue`) and
/// no longer implements serde `Serialize`, which Tauri requires for the `Err`
/// side of every command's `Result`. The orphan rule forbids implementing serde
/// for the foreign core type from this crate, so we mirror it here with
/// identical public fields and JSON shape. `commands.rs` imports this type as
/// `SerializableError`; construction sites (`SerializableError { .. }`) are
/// unchanged, and core errors convert in via the `From` impls below.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SerializableError {
    /// Error kind/variant name
    pub kind: String,
    /// Human-readable error message
    pub message: String,
    /// Associated path (if applicable)
    pub path: Option<std::path::PathBuf>,
}

impl From<diaryx_core::error::SerializableError> for SerializableError {
    fn from(e: diaryx_core::error::SerializableError) -> Self {
        Self {
            kind: e.kind,
            message: e.message,
            path: e.path,
        }
    }
}

impl From<&diaryx_core::error::DiaryxError> for SerializableError {
    fn from(e: &diaryx_core::error::DiaryxError) -> Self {
        diaryx_core::error::SerializableError::from(e).into()
    }
}

/// Convert a `serde_json::Value` (from JS) into a core fig type.
///
/// `fig::Value` implements serde `Deserialize`, so we deserialize the incoming
/// JSON into a `fig::Value` and then use the type's `FromValue` impl.
pub fn json_to_fig<T: diaryx_core::fig::FromValue>(
    val: serde_json::Value,
) -> Result<T, diaryx_core::fig::Error> {
    let fv: diaryx_core::fig::Value =
        serde_json::from_value(val).map_err(|e| diaryx_core::fig::Error::Message(e.to_string()))?;
    T::from_value(&fv)
}
