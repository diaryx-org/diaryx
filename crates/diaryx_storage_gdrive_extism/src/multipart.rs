//! Manual multipart/related encoding for Google Drive file uploads.

/// Build a multipart/related body for Google Drive file uploads.
///
/// Returns (content_type, body_bytes) where content_type includes the boundary.
pub fn build_multipart_upload(
    metadata_json: &str,
    content: &[u8],
    content_type: &str,
) -> (String, Vec<u8>) {
    let boundary = "diaryx_boundary_0123456789";
    let content_type_header = format!("multipart/related; boundary={boundary}");

    let mut body = Vec::with_capacity(metadata_json.len() + content.len() + 256);

    // Metadata part
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
    body.extend_from_slice(metadata_json.as_bytes());
    body.extend_from_slice(b"\r\n");

    // Content part
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(content);
    body.extend_from_slice(b"\r\n");

    // Closing boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    (content_type_header, body)
}
