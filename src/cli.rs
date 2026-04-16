use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "cnv",
    about = "Convert files between formats (PDF/DOCX/EPUB/PPTX/HTML → markdown, MD → Typst, SVG → PNG)",
    version
)]
pub struct Cli {
    /// Input: file path, directory, or glob pattern
    pub input: String,

    /// Output format (default depends on input type)
    #[arg(short, long, value_enum)]
    pub format: Option<Format>,

    /// Output directory (default: next to input file)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Target chunk size in approximate tokens (RAG format only)
    #[arg(long, default_value = "500")]
    pub chunk_size: usize,

    /// Minimum vertical gap for horizontal cuts in points (PDF XY-Cut tuning)
    #[arg(long, default_value = "8.0")]
    pub min_h_gap: f32,

    /// Minimum horizontal gap for vertical cuts in points (PDF XY-Cut tuning)
    #[arg(long, default_value = "12.0")]
    pub min_v_gap: f32,

    /// Skip image extraction
    #[arg(long)]
    pub no_images: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Paper size for Typst output (e.g. "a4", "us-letter")
    #[arg(long)]
    pub paper: Option<String>,

    /// Path to Typst config TOML file
    #[arg(long)]
    pub typst_config: Option<PathBuf>,

    /// Write output to stdout (Typst format only)
    #[arg(long)]
    pub stdout: bool,
}

#[derive(ValueEnum, Debug, Clone, PartialEq)]
pub enum Format {
    // Document → Markdown variants (Pipeline A)
    Raw,
    Rag,
    Karpathy,
    Kg,
    // Markdown → Typst (Pipeline B)
    Typst,
    // SVG → raster (Pipeline C)
    Png,
}

/// Detected input type based on file extension.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputType {
    Pdf,
    Docx,
    Epub,
    Pptx,
    Html,
    Markdown,
    Svg,
}

#[allow(dead_code)]
impl InputType {
    /// Detect input type from file extension.
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "pdf" => Some(Self::Pdf),
            "docx" => Some(Self::Docx),
            "epub" => Some(Self::Epub),
            "pptx" => Some(Self::Pptx),
            "html" | "htm" => Some(Self::Html),
            "md" | "markdown" => Some(Self::Markdown),
            "svg" => Some(Self::Svg),
            _ => None,
        }
    }

    /// Default output format for this input type.
    pub fn default_format(&self) -> Format {
        match self {
            Self::Pdf | Self::Docx | Self::Epub | Self::Pptx | Self::Html => Format::Raw,
            Self::Markdown => Format::Typst,
            Self::Svg => Format::Png,
        }
    }

    /// Check if a given output format is valid for this input type.
    pub fn supports_format(&self, format: &Format) -> bool {
        match self {
            Self::Pdf | Self::Docx | Self::Epub | Self::Pptx | Self::Html => {
                matches!(format, Format::Raw | Format::Rag | Format::Karpathy | Format::Kg)
            }
            Self::Markdown => matches!(format, Format::Typst),
            Self::Svg => matches!(format, Format::Png),
        }
    }

    /// File extensions associated with this input type.
    pub fn extensions(&self) -> &[&str] {
        match self {
            Self::Pdf => &["pdf"],
            Self::Docx => &["docx"],
            Self::Epub => &["epub"],
            Self::Pptx => &["pptx"],
            Self::Html => &["html", "htm"],
            Self::Markdown => &["md", "markdown"],
            Self::Svg => &["svg"],
        }
    }
}

/// All file extensions that cnv supports.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "epub", "pptx", "html", "htm", "md", "markdown", "svg",
];
