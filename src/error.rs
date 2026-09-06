//! Error types for pdfp operations.
//!
//! Library entry points (e.g. `pipeline::process_pdf`) return
//! [`PdfpResult`]. Specific, matchable errors are raised for known
//! conditions (unopenable files, encrypted PDFs, IO failures, invalid
//! input); everything else is carried transparently in
//! [`PdfpError::Other`] so the anyhow diagnostic chain is preserved.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PdfpError {
    #[error("Failed to open PDF '{path}': {message}")]
    PdfOpen { path: PathBuf, message: String },

    #[error("Failed to extract page {page}: {message}")]
    PdfExtraction { page: usize, message: String },

    #[error("IO error writing to '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Invalid input '{0}': {1}")]
    InvalidInput(String, String),

    #[error("PDF is password-protected, cannot process: {0}")]
    PasswordProtected(PathBuf),

    #[error("Hybrid backend ({url}) failed: {message}")]
    HybridBackend { url: String, message: String },

    /// An error raised by an internal stage. Transparently carries the
    /// anyhow diagnostic chain so `format!("{err:#}")` keeps full context.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Convenience result type for pdfp operations.
pub type PdfpResult<T> = Result<T, PdfpError>;
