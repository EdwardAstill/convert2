use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum VtvError {
    #[error("Failed to open PDF '{path}': {message}")]
    PdfOpen { path: PathBuf, message: String },

    #[error("Failed to extract page {page}: {message}")]
    PdfExtraction { page: usize, message: String },

    #[error("Page {0} has no extractable text (possibly a scanned image PDF)")]
    EmptyPage(usize),

    #[error("Layout analysis failed on page {page}: {message}")]
    LayoutAnalysis { page: usize, message: String },

    #[error("IO error writing to '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid input '{0}': {1}")]
    InvalidInput(String, String),

    #[error("PDF is password-protected, cannot process: {0}")]
    PasswordProtected(PathBuf),

    #[error("Failed to parse DOCX '{path}': {message}")]
    DocxParse { path: PathBuf, message: String },

    #[error("Failed to parse EPUB '{path}': {message}")]
    EpubParse { path: PathBuf, message: String },

    #[error("Failed to parse PPTX '{path}': {message}")]
    PptxParse { path: PathBuf, message: String },

    #[error("Failed to parse HTML '{path}': {message}")]
    HtmlParse { path: PathBuf, message: String },

    #[error("Failed to convert SVG '{path}': {message}")]
    SvgConvert { path: PathBuf, message: String },

    #[error("Failed to convert Markdown to Typst: {0}")]
    TypstConvert(String),

    #[error("Unsupported conversion: {0}")]
    UnsupportedConversion(String),

    #[error("Hybrid backend ({url}) failed: {message}")]
    HybridBackend { url: String, message: String },
}

/// Convenience result type for vtv operations.
pub type VtvResult<T> = Result<T, VtvError>;
