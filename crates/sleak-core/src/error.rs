use std::path::PathBuf;
use thiserror::Error;

/// Every failure mode a Sleak analysis run can hit, with enough context
/// in each variant that a caller (CLI or agent) never needs to re-run
/// anything to act on it.
#[derive(Debug, Error)]
pub enum SleakError {
    #[error("{path} is not a Cargo workspace root (no Cargo.toml found)")]
    NotACargoWorkspace { path: PathBuf },

    #[error("failed to run `cargo metadata` in {path}: {reason}")]
    MetadataFailure { path: PathBuf, reason: String },

    #[error("failed to parse {file}: {reason}")]
    ParseFailure { file: PathBuf, reason: String },

    #[error("invalid configuration at {path}: {reason}")]
    Configuration { path: PathBuf, reason: String },

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
