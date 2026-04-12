use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "vtv",
    about = "Convert PDFs to AI-friendly markdown",
    version
)]
pub struct Cli {
    /// Input: PDF file, directory, or glob pattern (e.g. "*.pdf")
    pub input: String,

    /// Output format
    #[arg(short, long, value_enum, default_value = "raw")]
    pub format: Format,

    /// Output directory (default: next to input file)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Target chunk size in approximate tokens (RAG format only)
    #[arg(long, default_value = "500")]
    pub chunk_size: usize,

    /// Minimum vertical gap for horizontal cuts in points (XY-Cut tuning)
    #[arg(long, default_value = "8.0")]
    pub min_h_gap: f32,

    /// Minimum horizontal gap for vertical cuts in points (XY-Cut tuning)
    #[arg(long, default_value = "12.0")]
    pub min_v_gap: f32,

    /// Skip image extraction
    #[arg(long)]
    pub no_images: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(ValueEnum, Debug, Clone, PartialEq)]
pub enum Format {
    Raw,
    Rag,
    Karpathy,
    Kg,
}
