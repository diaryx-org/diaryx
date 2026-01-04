//! Cloud backup targets for Tauri app.
//!
//! Implements cloud storage backends (S3, Google Drive, etc.) for the backup system.

mod google_drive;
mod s3;

pub use google_drive::GoogleDriveTarget;
pub use s3::S3Target;
