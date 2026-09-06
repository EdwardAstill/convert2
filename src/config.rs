//! Conversion configuration types.
//!
//! This module holds the semantic configuration for the PDF → Markdown
//! conversion pipeline. It is intentionally free of any CLI dependency:
//! [`cli`](crate::cli) defines the clap parsing layer and converts its
//! argument types into the plain structs and enums defined here, while
//! `pipeline`, `layout`, and `ocr` consume only this module.

use std::path::PathBuf;

use crate::error::PdfpResult;
use crate::render::markdown::MarkdownStyle;

/// OCR preprocessing configuration shared by convert/inspect/search flows.
#[derive(Debug, Clone)]
pub struct OcrOptions {
    /// OCR preprocessing mode. `auto` OCRs scan-heavy PDFs only; `force`
    /// OCRs regardless of readable text; `off` skips OCR.
    pub ocr: OcrMode,

    /// OCR language(s), passed to OCRmyPDF/Tesseract, e.g. `eng` or `eng+deu`.
    pub ocr_lang: String,

    /// Optional cache directory for derived searchable PDFs.
    pub ocr_cache_dir: Option<PathBuf>,

    /// Timeout in seconds for OCR preprocessing.
    pub ocr_timeout_secs: u64,

    /// OCRmyPDF executable path or command name.
    pub ocr_command: PathBuf,
}

impl Default for OcrOptions {
    fn default() -> Self {
        Self {
            ocr: OcrMode::Auto,
            ocr_lang: "eng".to_string(),
            ocr_cache_dir: None,
            ocr_timeout_secs: 600,
            ocr_command: PathBuf::from("ocrmypdf"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrMode {
    Off,
    Auto,
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureMode {
    /// Current behavior: extract embedded raster image objects
    Embedded,
    /// Render complete detected figure regions as page snapshots
    Snapshot,
    /// Emit both rendered figure snapshots and embedded image objects
    Both,
    /// Do not emit image or figure assets
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableMode {
    /// Detect tables automatically; emit Markdown when confident, layout text otherwise
    Auto,
    /// Force native coordinate-derived Markdown tables
    Native,
    /// Preserve detected table regions as fenced fixed-width layout text
    Layout,
    /// Disable coordinate table reconstruction
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaMode {
    /// Detect formula candidates for warnings and debug audit files
    Auto,
    /// Force local formula candidate detection and rendering
    Local,
    /// Detect formula candidates for hybrid backend routing and audit
    Hybrid,
    /// Disable formula candidate detection
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaEmitMode {
    /// Emit only candidates that pass conservative safety gates.
    Conservative,
    /// Emit high-confidence local candidates and recovered sidecar LaTeX.
    Auto,
    /// Emit every non-empty detected or recovered formula candidate.
    All,
    /// Never emit formula blocks; keep audit/debug records only.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridMode {
    Off,
    Docling,
}

impl HybridMode {
    pub fn is_on(self) -> bool {
        !matches!(self, HybridMode::Off)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridPolicy {
    /// Triage per page based on math-symbol count, table presence, and text
    /// density — only formula-/table-/scan-heavy pages pay the backend cost.
    Auto,
    /// Route every page (useful for testing).
    All,
}

/// Parsed `--formula-sidecar` value.
#[derive(Debug, Clone)]
pub enum FormulaSidecarArg {
    Command(String),
    #[cfg(feature = "onnx-ocr")]
    Onnx(PathBuf),
}

/// Parse the `--formula-sidecar` CLI string into a [`FormulaSidecarArg`].
pub fn parse_formula_sidecar(value: &str) -> PdfpResult<FormulaSidecarArg> {
    #[cfg(feature = "onnx-ocr")]
    if let Some(model_dir) = value.strip_prefix("onnx:") {
        return Ok(FormulaSidecarArg::Onnx(PathBuf::from(model_dir)));
    }

    #[cfg(not(feature = "onnx-ocr"))]
    if value.starts_with("onnx:") {
        return Err(crate::error::PdfpError::InvalidInput(
            "formula sidecar".to_string(),
            "onnx formula sidecar requires a binary built with --features onnx-ocr".to_string(),
        ));
    }

    let command = value.strip_prefix("cmd:").unwrap_or(value);
    Ok(FormulaSidecarArg::Command(command.to_string()))
}

/// Configuration for a PDF → Markdown conversion run.
///
/// Constructed from CLI arguments (see `crate::cli`) or programmatically via
/// [`ConvertOptions::default`]. The `effective_*` helpers resolve mode
/// presets (conservative mode, review style) into concrete behaviours.
#[derive(Debug, Clone)]
pub struct ConvertOptions {
    /// Output directory (default: input file directory)
    pub output: Option<PathBuf>,

    /// Minimum vertical gap for horizontal cuts in points (PDF XY-Cut tuning)
    pub min_h_gap: f32,

    /// Minimum horizontal gap for vertical cuts in points (PDF XY-Cut tuning)
    pub min_v_gap: f32,

    /// Save detected figures and images under images/
    pub images: bool,

    /// Skip image extraction.
    pub no_images: bool,

    /// Prefer audit/fallback output over heuristic reconstruction.
    ///
    /// Conservative mode avoids speculative Markdown tables, formula rendering,
    /// and rendered figure snapshots. It is a preset for review-safe conversion;
    /// debug flags may still be used to inspect candidates.
    pub conservative: bool,

    /// Markdown output style: faithful extraction, clean reader-friendly
    /// Markdown, or review/audit output.
    pub markdown_style: MarkdownStyle,

    /// Figure/image output mode for markdown conversion
    pub figures: Option<FigureMode>,

    /// Resolution for rendered figure snapshots
    pub figure_dpi: u32,

    /// Padding around detected figure regions, in PDF points
    pub figure_padding: f32,

    /// Write figure candidate debug JSON under debug/figures/
    pub debug_figures: bool,

    /// Save detected table crops under tables/
    pub tables: bool,

    /// Table extraction mode for markdown conversion.
    pub table_mode: TableMode,

    /// Write table detection debug JSON under debug/tables/
    pub debug_tables: bool,

    /// Save detected equation crops under equations/
    pub equations: bool,

    /// Formula handling mode for markdown conversion
    pub formulas: FormulaMode,

    /// Write formula detection debug JSON and crops under debug/formulas/
    pub debug_formulas: bool,

    /// Optional formula OCR sidecar string. Use a command, cmd:<command>,
    /// or onnx:<model-dir>. Parsed by [`parse_formula_sidecar`].
    pub formula_sidecar: Option<String>,

    /// Formula sidecar timeout per crop, in seconds.
    pub formula_sidecar_timeout_secs: u64,

    /// Formula emission policy for detected/recovered candidates.
    pub formula_emit: FormulaEmitMode,

    /// Optional 1-indexed page range to convert, e.g. `1-3,9`.
    pub pages: Option<String>,

    /// Verbose output
    pub verbose: bool,

    /// Route PDFs through an external backend for higher-quality extraction
    /// (LaTeX formulas, complex tables, OCR). `off` (default) = fully local;
    /// `docling` = POST the whole PDF to a running `docling-serve` instance.
    pub hybrid: HybridMode,

    /// Base URL of the hybrid backend (docling-serve). Only used when
    /// `hybrid` is not `off`.
    pub hybrid_url: String,

    /// Timeout in seconds for the hybrid backend call. Large scanned PDFs on
    /// CPU can take minutes.
    pub hybrid_timeout_secs: u64,

    /// Which pages to route through the hybrid backend.
    pub hybrid_policy: HybridPolicy,

    /// Optional directory for cached hybrid markdown, keyed by source PDF
    /// metadata and page number.
    pub hybrid_cache_dir: Option<PathBuf>,

    /// OCR preprocessing configuration.
    pub ocr: OcrOptions,

    /// True when a conversion batch covers more than one input file.
    pub batch_mode: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            output: None,
            min_h_gap: 8.0,
            min_v_gap: 12.0,
            images: false,
            no_images: false,
            conservative: false,
            markdown_style: MarkdownStyle::Clean,
            figures: None,
            figure_dpi: 200,
            figure_padding: 8.0,
            debug_figures: false,
            tables: false,
            table_mode: TableMode::Auto,
            debug_tables: false,
            equations: false,
            formulas: FormulaMode::Auto,
            debug_formulas: false,
            formula_sidecar: None,
            formula_sidecar_timeout_secs: 30,
            formula_emit: FormulaEmitMode::Auto,
            pages: None,
            verbose: false,
            hybrid: HybridMode::Off,
            hybrid_url: "http://localhost:5001".to_string(),
            hybrid_timeout_secs: 600,
            hybrid_policy: HybridPolicy::Auto,
            hybrid_cache_dir: None,
            ocr: OcrOptions::default(),
            batch_mode: false,
        }
    }
}

impl ConvertOptions {
    pub fn review_safe_profile(&self) -> bool {
        self.conservative || matches!(self.markdown_style, MarkdownStyle::Review)
    }

    pub fn effective_figure_mode(&self) -> FigureMode {
        if self.review_safe_profile() {
            FigureMode::Embedded
        } else {
            self.figures.unwrap_or(FigureMode::Snapshot)
        }
    }

    pub fn effective_image_output(&self) -> bool {
        !self.no_images
            && !matches!(self.figures, Some(FigureMode::None))
            && (self.images || self.figures.is_some())
    }

    pub fn effective_table_mode(&self) -> TableMode {
        if self.review_safe_profile() {
            TableMode::Layout
        } else {
            self.table_mode
        }
    }

    pub fn export_table_images(&self) -> bool {
        self.tables && !matches!(self.effective_table_mode(), TableMode::Off)
    }

    pub fn export_equation_images(&self) -> bool {
        self.equations
    }

    pub fn effective_formula_mode(&self) -> FormulaMode {
        if self.review_safe_profile() {
            FormulaMode::Auto
        } else {
            self.formulas
        }
    }

    pub fn effective_render_math(&self) -> bool {
        !self.review_safe_profile()
    }

    pub fn effective_markdown_style(&self) -> MarkdownStyle {
        self.markdown_style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_options(conservative: bool) -> ConvertOptions {
        ConvertOptions {
            output: None,
            min_h_gap: 8.0,
            min_v_gap: 12.0,
            images: true,
            no_images: false,
            conservative,
            markdown_style: MarkdownStyle::Clean,
            figures: Some(FigureMode::Snapshot),
            figure_dpi: 200,
            figure_padding: 8.0,
            debug_figures: false,
            tables: true,
            table_mode: TableMode::Native,
            debug_tables: false,
            equations: false,
            formulas: FormulaMode::Local,
            debug_formulas: false,
            formula_sidecar: None,
            formula_sidecar_timeout_secs: 30,
            formula_emit: FormulaEmitMode::Auto,
            pages: None,
            verbose: false,
            hybrid: HybridMode::Off,
            hybrid_url: "http://localhost:5001".to_string(),
            hybrid_timeout_secs: 600,
            hybrid_policy: HybridPolicy::Auto,
            hybrid_cache_dir: None,
            ocr: OcrOptions::default(),
            batch_mode: false,
        }
    }

    #[test]
    fn default_markdown_style_is_clean() {
        assert_eq!(
            ConvertOptions::default().markdown_style,
            MarkdownStyle::Clean
        );
    }

    #[test]
    fn conservative_mode_uses_review_safe_conversion_modes() {
        let options = convert_options(true);

        assert_eq!(options.effective_figure_mode(), FigureMode::Embedded);
        assert_eq!(options.effective_table_mode(), TableMode::Layout);
        assert_eq!(options.effective_formula_mode(), FormulaMode::Auto);
    }

    #[test]
    fn non_conservative_mode_preserves_selected_conversion_modes() {
        let options = convert_options(false);

        assert_eq!(options.effective_figure_mode(), FigureMode::Snapshot);
        assert_eq!(options.effective_table_mode(), TableMode::Native);
        assert_eq!(options.effective_formula_mode(), FormulaMode::Local);
    }

    #[test]
    fn review_style_uses_review_safe_conversion_modes() {
        let mut options = convert_options(false);
        options.markdown_style = MarkdownStyle::Review;

        assert_eq!(options.effective_figure_mode(), FigureMode::Embedded);
        assert_eq!(options.effective_table_mode(), TableMode::Layout);
        assert_eq!(options.effective_formula_mode(), FormulaMode::Auto);
        assert!(!options.effective_render_math());
    }

    #[test]
    fn clean_style_keeps_selected_table_mode_without_disabling_math_rendering() {
        let mut options = convert_options(false);
        options.markdown_style = MarkdownStyle::Clean;
        options.table_mode = TableMode::Layout;

        assert_eq!(options.effective_table_mode(), TableMode::Layout);
        assert!(options.effective_render_math());
    }
}
