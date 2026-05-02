//! Error types for `bookmatter`.

use thiserror::Error;

/// Errors that can occur while parsing or serializing markdown frontmatter.
#[derive(Debug, Error)]
pub enum FrontmatterError {
    /// The input did not contain valid frontmatter delimiters (`---` ... `---`).
    #[error("file has no frontmatter delimiters")]
    NoFrontmatter,

    /// The frontmatter YAML failed to parse or serialize.
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
}

/// Convenience `Result` alias parameterized by [`FrontmatterError`].
pub type Result<T> = std::result::Result<T, FrontmatterError>;
