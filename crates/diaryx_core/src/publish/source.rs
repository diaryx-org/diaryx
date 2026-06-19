//! Value types describing the audience-scoped sources a publish uploads.
//!
//! These are the *inputs* to [`super::service::PublishService`]. A collector
//! (the workspace walk) produces them; the service diffs + uploads them. Keeping
//! them as plain data lets the orchestration be unit-tested without a workspace.

/// A collected, audience-scoped markdown source ready to upload.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Audience-scoped markdown source: sensitive frontmatter stripped, body
    /// visibility-filtered. Uploaded as the source object; the server renders it.
    pub source_markdown: String,
    /// Sanitized workspace-relative `.md` path (e.g. `"Welcome.md"`,
    /// `"notes/post.md"`). Used as the source object key so frontmatter
    /// `contents`/`part_of` links resolve server-side.
    pub source_rel_path: String,
    /// Sanitized destination `.html` path the ARK resolves to (e.g.
    /// `"index.html"`, `"notes/post.html"`) — the HTML the server build creates.
    pub dest_path: String,
    /// The page's ARK blade (frontmatter `id`), if any.
    pub file_ark: Option<String>,
    /// Whether this is the workspace root/index page.
    pub is_index: bool,
}

/// A prepared attachment upload (already read + any transform applied).
#[derive(Debug, Clone)]
pub struct Attachment {
    /// Destination path relative to the audience root (e.g.
    /// `"_attachments/image.png"`).
    pub dest_rel: String,
    /// File bytes to upload.
    pub bytes: Vec<u8>,
    /// MIME type.
    pub mime_type: String,
}

/// Everything a single audience contributes to a publish.
#[derive(Debug, Clone)]
pub struct AudienceInput {
    /// Audience name (object key prefix).
    pub name: String,
    /// Gate stack JSON (empty array = public).
    pub gates: serde_json::Value,
    /// `false` = legacy "unpublished": delete all the audience's objects and
    /// upload nothing.
    pub publish: bool,
    /// Collected markdown sources for this audience.
    pub sources: Vec<SourceFile>,
    /// Prepared attachments for this audience.
    pub attachments: Vec<Attachment>,
}

impl AudienceInput {
    /// An audience is the public front-page audience when its gate stack is
    /// empty (no access restrictions).
    pub fn is_public(&self) -> bool {
        self.gates.as_array().map(|a| a.is_empty()).unwrap_or(false)
    }
}
