use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::render::markdown::MarkdownStyle as RenderMarkdownStyle;

#[derive(Parser, Debug)]
#[command(name = "pdfp", about = "Local PDF processor", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Input PDF: file path, directory, or glob pattern.
    ///
    /// Backwards-compatible alias for `pdfp convert <INPUT>`.
    pub input: Option<String>,

    #[command(flatten)]
    pub convert_options: ConvertOptions,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Convert PDF files into markdown
    Convert(ConvertArgs),
    /// Create a searchable OCR PDF
    Ocr(OcrArgs),
    /// Check installed and bundled runtime dependencies
    Doctor(DoctorArgs),
    /// Inspect PDF metadata, page sizes, and scan-like signals
    Inspect(InspectArgs),
    /// Read, set, and clear document information metadata
    Metadata(MetadataCommand),
    /// Search PDF text and report matching pages
    Search(SearchArgs),
    /// Run quality evaluation against fixture JSON files
    Eval(EvalArgs),
    /// Page extraction, deletion, splitting, reordering, and merging
    Pages(PagesCommand),
    /// Print imposition operations such as 2-up and booklet output
    Impose(ImposeCommand),
    /// Page-level editing operations
    Page(PageCommand),
    /// Update pdfp to the latest GitHub release
    Update(UpdateArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ConvertArgs {
    /// Input PDF: file path, directory, or glob pattern
    pub input: String,

    #[command(flatten)]
    pub options: ConvertOptions,
}

#[derive(Args, Debug, Clone)]
pub struct ConvertOptions {
    /// Output directory (default: input file directory)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Minimum vertical gap for horizontal cuts in points (PDF XY-Cut tuning)
    #[arg(long, default_value = "8.0", hide = true)]
    pub min_h_gap: f32,

    /// Minimum horizontal gap for vertical cuts in points (PDF XY-Cut tuning)
    #[arg(long, default_value = "12.0", hide = true)]
    pub min_v_gap: f32,

    /// Save detected figures and images under images/
    #[arg(long)]
    pub images: bool,

    /// Skip image extraction.
    #[arg(long, hide = true)]
    pub no_images: bool,

    /// Prefer audit/fallback output over heuristic reconstruction.
    ///
    /// Conservative mode avoids speculative Markdown tables, formula rendering,
    /// and rendered figure snapshots. It is a preset for review-safe conversion;
    /// debug flags may still be used to inspect candidates.
    #[arg(long, hide = true)]
    pub conservative: bool,

    /// Markdown output style: faithful extraction, clean reader-friendly Markdown, or review/audit output.
    #[arg(
        long = "markdown-style",
        alias = "format",
        value_enum,
        default_value = "clean",
        hide = true
    )]
    pub markdown_style: MarkdownStyleArg,

    /// Figure/image output mode for markdown conversion
    #[arg(long, value_enum, hide = true)]
    pub figures: Option<FigureMode>,

    /// Resolution for rendered figure snapshots
    #[arg(long, default_value = "200", hide = true)]
    pub figure_dpi: u32,

    /// Padding around detected figure regions, in PDF points
    #[arg(long, default_value = "8.0", hide = true)]
    pub figure_padding: f32,

    /// Write figure candidate debug JSON under debug/figures/
    #[arg(long, hide = true)]
    pub debug_figures: bool,

    /// Save detected table crops under tables/
    #[arg(long)]
    pub tables: bool,

    /// Table extraction mode for markdown conversion.
    #[arg(long = "table-mode", value_enum, default_value = "auto", hide = true)]
    pub table_mode: TableMode,

    /// Write table detection debug JSON under debug/tables/
    #[arg(long, hide = true)]
    pub debug_tables: bool,

    /// Save detected equation crops under equations/
    #[arg(long)]
    pub equations: bool,

    /// Formula handling mode for markdown conversion
    #[arg(long, value_enum, default_value = "auto", hide = true)]
    pub formulas: FormulaMode,

    /// Write formula detection debug JSON and crops under debug/formulas/
    #[arg(long, hide = true)]
    pub debug_formulas: bool,

    /// Optional formula OCR sidecar. Use a command, cmd:<command>, or onnx:<model-dir>.
    #[arg(long, value_name = "SIDECAR", hide = true)]
    pub formula_sidecar: Option<String>,

    /// Formula sidecar timeout per crop, in seconds.
    #[arg(long, default_value = "30", hide = true)]
    pub formula_sidecar_timeout_secs: u64,

    /// Formula emission policy for detected/recovered candidates.
    #[arg(long = "formula-emit", value_enum, default_value = "auto", hide = true)]
    pub formula_emit: FormulaEmitMode,

    /// Optional 1-indexed page range to convert, e.g. `1-3,9`.
    #[arg(long)]
    pub pages: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Route PDFs through an external backend for higher-quality extraction
    /// (LaTeX formulas, complex tables, OCR). `off` (default) = fully local;
    /// `docling` = POST the whole PDF to a running `docling-serve` instance.
    #[arg(long, value_enum, default_value = "off", hide = true)]
    pub hybrid: HybridMode,

    /// Base URL of the hybrid backend (docling-serve). Only used when
    /// `--hybrid` is not `off`.
    #[arg(long, default_value = "http://localhost:5001", hide = true)]
    pub hybrid_url: String,

    /// Timeout in seconds for the hybrid backend call. Large scanned PDFs on
    /// CPU can take minutes.
    #[arg(long, default_value = "600", hide = true)]
    pub hybrid_timeout_secs: u64,

    /// Which pages to route through the hybrid backend. `auto` (default)
    /// triages per page based on math-symbol count, table presence, and
    /// text density — only formula-/table-/scan-heavy pages pay the
    /// backend cost. `all` routes every page (useful for testing).
    #[arg(long, value_enum, default_value = "auto", hide = true)]
    pub hybrid_policy: HybridPolicy,

    /// Optional directory for cached hybrid markdown, keyed by source PDF
    /// metadata and page number.
    #[arg(long, hide = true)]
    pub hybrid_cache_dir: Option<PathBuf>,

    #[command(flatten)]
    pub ocr: OcrOptions,

    #[arg(skip)]
    pub batch_mode: bool,
}

#[derive(Args, Debug)]
pub struct InspectArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Emit machine-readable JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    #[command(flatten)]
    pub ocr: OcrOptions,
}

#[derive(Args, Debug)]
pub struct MetadataCommand {
    #[command(subcommand)]
    pub command: MetadataSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum MetadataSubcommand {
    /// Show document information metadata
    Show(MetadataShowArgs),
    /// Set document information metadata and write a new PDF
    Set(MetadataSetArgs),
    /// Clear selected document information metadata fields and write a new PDF
    Clear(MetadataClearArgs),
}

#[derive(Args, Debug)]
pub struct MetadataShowArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Emit machine-readable JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Include PDF version and XMP/signature status in human output
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args, Debug)]
pub struct MetadataSetArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,

    /// Set the document title
    #[arg(long)]
    pub title: Option<String>,

    /// Set the document author
    #[arg(long)]
    pub author: Option<String>,

    /// Set the document subject
    #[arg(long)]
    pub subject: Option<String>,

    /// Set document keywords
    #[arg(long)]
    pub keywords: Option<String>,

    /// Set the document creator
    #[arg(long)]
    pub creator: Option<String>,

    /// Set the document producer
    #[arg(long)]
    pub producer: Option<String>,

    /// Set creation date as `now`, RFC3339, or a PDF date such as `D:20260519123000Z`
    #[arg(long)]
    pub creation_date: Option<String>,

    /// Set modification date as `now`, RFC3339, or a PDF date such as `D:20260519123000Z`
    #[arg(long)]
    pub mod_date: Option<String>,

    /// Do not automatically update ModDate when setting other fields
    #[arg(long)]
    pub no_touch_mod_date: bool,

    /// Allow writing PDFs that appear to contain signature fields
    #[arg(long)]
    pub force_signed: bool,

    /// Emit machine-readable JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Print changed field names to stderr
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args, Debug)]
pub struct MetadataClearArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,

    /// Fields to clear. Use a comma list, for example `--fields title,author`.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1.., required = true)]
    pub fields: Vec<MetadataField>,

    /// Allow writing PDFs that appear to contain signature fields
    #[arg(long)]
    pub force_signed: bool,

    /// Emit machine-readable JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Print cleared field names to stderr
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataField {
    Title,
    Author,
    Subject,
    Keywords,
    Creator,
    Producer,
    CreationDate,
    #[value(name = "mod-date", alias = "modification-date")]
    ModDate,
    All,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Text to search for
    pub needle: String,

    /// Optional 1-indexed page range to search, e.g. `1-3,9`
    #[arg(long)]
    pub pages: Option<String>,

    /// Emit machine-readable JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Number of surrounding characters to include in human output.
    ///
    /// The first implementation reports page numbers and hit boxes; context is
    /// accepted now so the CLI contract does not need to change later.
    #[arg(long, default_value = "0")]
    pub context: usize,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    #[command(flatten)]
    pub ocr: OcrOptions,
}

#[derive(Args, Debug)]
pub struct EvalArgs {
    /// Directory containing fixture JSON files and their PDFs
    pub dir: PathBuf,
}

#[derive(Args, Debug)]
pub struct OcrArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Output searchable PDF
    #[arg(short, long)]
    pub output: PathBuf,

    /// OCR mode. `auto` skips born-digital/readable pages; `force` rasterizes
    /// all pages and rebuilds the text layer.
    #[arg(long, value_enum, default_value = "auto")]
    pub mode: StandaloneOcrMode,

    /// OCR language(s), passed to OCRmyPDF/Tesseract, e.g. `eng` or `eng+deu`.
    #[arg(long, alias = "ocr-lang", default_value = "eng")]
    pub lang: String,

    /// Optional cache directory for derived searchable PDFs.
    #[arg(long, alias = "ocr-cache-dir")]
    pub cache_dir: Option<PathBuf>,

    /// Timeout in seconds for OCR preprocessing.
    #[arg(long, alias = "ocr-timeout-secs", default_value = "600")]
    pub timeout_secs: u64,

    /// OCRmyPDF executable path or command name.
    #[arg(long, alias = "ocr-command", default_value = "ocrmypdf")]
    pub command: PathBuf,

    /// Emit machine-readable OCR decision JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Emit machine-readable JSON to stdout
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct PagesCommand {
    #[command(subcommand)]
    pub command: PagesSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum PagesSubcommand {
    /// Extract selected pages into a new PDF
    Extract(PageSelectionArgs),
    /// Delete selected pages and write a new PDF
    Delete(PageSelectionArgs),
    /// Split a PDF into chunks
    Split(SplitArgs),
    /// Reorder pages into a new PDF
    Reorder(PageSelectionArgs),
    /// Merge PDFs
    Merge(MergeArgs),
    /// Set page rotation on selected pages
    Rotate(RotateArgs),
}

#[derive(Args, Debug)]
pub struct MergeArgs {
    /// Input PDFs, in output order
    #[arg(required = true, num_args = 1..)]
    pub inputs: Vec<PathBuf>,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(Args, Debug)]
pub struct PageSelectionArgs {
    /// Input PDF
    pub input: PathBuf,

    /// 1-indexed page range, e.g. `1-3,9`, `odd`, `even`, or `all`
    #[arg(long)]
    pub pages: String,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(Args, Debug)]
pub struct RotateArgs {
    /// Input PDF
    pub input: PathBuf,

    /// 1-indexed page range, e.g. `1-3,9`, `odd`, `even`, or `all`
    #[arg(long, default_value = "all")]
    pub pages: String,

    /// Absolute page rotation in degrees. Must be a multiple of 90.
    #[arg(long)]
    pub degrees: i32,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(Args, Debug)]
pub struct SplitArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Number of pages per output chunk
    #[arg(long)]
    pub every: usize,

    /// Output directory
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(Args, Debug)]
pub struct ImposeCommand {
    #[command(subcommand)]
    pub command: ImposeSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum ImposeSubcommand {
    /// Compose two source pages onto one output page
    #[command(name = "2up")]
    TwoUp(ImposeArgs),
    /// Create booklet imposition output
    Booklet(ImposeArgs),
}

#[derive(Args, Debug)]
pub struct ImposeArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(Args, Debug)]
pub struct PageCommand {
    #[command(subcommand)]
    pub command: PageSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum PageSubcommand {
    /// Resize pages to a target paper size
    Resize(ResizeArgs),
    /// Set CropBox on selected pages
    Crop(CropArgs),
    /// Add a searchable text overlay to selected pages
    Text(PageTextArgs),
}

#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Just check for a newer version; don't install
    #[arg(long)]
    pub check: bool,

    /// Force reinstall even if the same version
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct ResizeArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,

    /// Target paper size
    #[arg(long, default_value = "a4")]
    pub paper: String,

    /// Fit mode: contain, cover, or stretch
    #[arg(long, default_value = "contain")]
    pub fit: String,
}

#[derive(Args, Debug)]
pub struct CropArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,

    /// 1-indexed page range, e.g. `1-3,9`, `odd`, `even`, or `all`
    #[arg(long, default_value = "all")]
    pub pages: String,

    /// Crop box as x0 y0 x1 y1 in PDF points
    #[arg(long = "box", num_args = 4, value_names = ["X0", "Y0", "X1", "Y1"])]
    pub crop_box: Vec<f32>,
}

#[derive(Args, Debug)]
pub struct PageTextArgs {
    /// Input PDF
    pub input: PathBuf,

    /// Output PDF
    #[arg(short, long)]
    pub output: PathBuf,

    /// Text to add. Newlines create additional lines.
    #[arg(long)]
    pub text: String,

    /// 1-indexed page range, e.g. `1-3,9`, `odd`, `even`, or `all`
    #[arg(long, default_value = "all")]
    pub pages: String,

    /// Horizontal baseline position from the selected origin, in PDF points
    #[arg(long)]
    pub x: f32,

    /// Vertical baseline position from the selected origin, in PDF points
    #[arg(long)]
    pub y: f32,

    /// Coordinate origin used by --x and --y
    #[arg(long, value_enum, default_value = "bottom-left")]
    pub origin: TextOrigin,

    /// Built-in PDF font
    #[arg(long, value_enum, default_value = "helvetica")]
    pub font: PdfTextFont,

    /// Font size in PDF points
    #[arg(long = "font-size", alias = "size", default_value = "12")]
    pub font_size: f32,

    /// Text colour: a name or #RRGGBB/#RGB
    #[arg(long = "color", alias = "colour", default_value = "#000000")]
    pub color: String,

    /// Distance between multiline baselines in points (default: 1.2 × font size)
    #[arg(long)]
    pub line_height: Option<f32>,

    /// Allow writing PDFs that appear to contain signature fields
    #[arg(long)]
    pub force_signed: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOrigin {
    BottomLeft,
    TopLeft,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfTextFont {
    Helvetica,
    HelveticaBold,
    HelveticaOblique,
    HelveticaBoldOblique,
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
}

impl Cli {
    pub fn into_command(self) -> anyhow::Result<AppCommand> {
        match self.command {
            Some(Command::Convert(args)) => Ok(AppCommand::Convert(args)),
            Some(Command::Ocr(args)) => Ok(AppCommand::Ocr(args)),
            Some(Command::Doctor(args)) => Ok(AppCommand::Doctor(args)),
            Some(Command::Inspect(args)) => Ok(AppCommand::Inspect(args)),
            Some(Command::Metadata(args)) => Ok(AppCommand::Metadata(args)),
            Some(Command::Search(args)) => Ok(AppCommand::Search(args)),
            Some(Command::Eval(args)) => Ok(AppCommand::Eval(args)),
            Some(Command::Pages(args)) => Ok(AppCommand::Pages(args)),
            Some(Command::Impose(args)) => Ok(AppCommand::Impose(args)),
            Some(Command::Page(args)) => Ok(AppCommand::Page(args)),
            Some(Command::Update(args)) => Ok(AppCommand::Update(args)),
            None => {
                let input = self.input.ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing input; use `pdfp <INPUT>` or a subcommand such as `pdfp inspect <INPUT>`"
                    )
                })?;
                Ok(AppCommand::Convert(ConvertArgs {
                    input,
                    options: self.convert_options,
                }))
            }
        }
    }
}

#[derive(Debug)]
pub enum AppCommand {
    Convert(ConvertArgs),
    Ocr(OcrArgs),
    Doctor(DoctorArgs),
    Inspect(InspectArgs),
    Metadata(MetadataCommand),
    Search(SearchArgs),
    Eval(EvalArgs),
    Pages(PagesCommand),
    Impose(ImposeCommand),
    Page(PageCommand),
    Update(UpdateArgs),
}

#[derive(Args, Debug, Clone)]
pub struct OcrOptions {
    /// OCR preprocessing mode. `auto` OCRs scan-heavy PDFs only; `force`
    /// OCRs regardless of readable text; `off` skips OCR.
    #[arg(long, value_enum, default_value = "auto")]
    pub ocr: OcrMode,

    /// OCR language(s), passed to OCRmyPDF/Tesseract, e.g. `eng` or `eng+deu`.
    #[arg(long = "lang", alias = "ocr-lang", default_value = "eng")]
    pub ocr_lang: String,

    /// Optional cache directory for derived searchable PDFs.
    #[arg(long, hide = true)]
    pub ocr_cache_dir: Option<PathBuf>,

    /// Timeout in seconds for OCR preprocessing.
    #[arg(long, default_value = "600", hide = true)]
    pub ocr_timeout_secs: u64,

    /// OCRmyPDF executable path or command name.
    #[arg(long, default_value = "ocrmypdf", hide = true)]
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrMode {
    Off,
    Auto,
    Force,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormulaSidecarArg {
    Command(String),
    #[cfg(feature = "onnx-ocr")]
    Onnx(PathBuf),
}

pub fn parse_formula_sidecar(value: &str) -> anyhow::Result<FormulaSidecarArg> {
    #[cfg(feature = "onnx-ocr")]
    if let Some(model_dir) = value.strip_prefix("onnx:") {
        return Ok(FormulaSidecarArg::Onnx(PathBuf::from(model_dir)));
    }

    #[cfg(not(feature = "onnx-ocr"))]
    if value.starts_with("onnx:") {
        anyhow::bail!("onnx formula sidecar requires a binary built with --features onnx-ocr");
    }

    let command = value.strip_prefix("cmd:").unwrap_or(value);
    Ok(FormulaSidecarArg::Command(command.to_string()))
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownStyleArg {
    /// Keep current PDF-faithful extraction output.
    Faithful,
    /// Produce reader-friendly Markdown by normalizing PDF layout artefacts.
    Clean,
    /// Prefer audit-safe output with conservative extraction choices.
    Review,
}

impl From<MarkdownStyleArg> for RenderMarkdownStyle {
    fn from(style: MarkdownStyleArg) -> Self {
        match style {
            MarkdownStyleArg::Faithful => RenderMarkdownStyle::Faithful,
            MarkdownStyleArg::Clean => RenderMarkdownStyle::Clean,
            MarkdownStyleArg::Review => RenderMarkdownStyle::Review,
        }
    }
}

impl ConvertOptions {
    pub fn review_safe_profile(&self) -> bool {
        self.conservative || matches!(self.markdown_style, MarkdownStyleArg::Review)
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

    pub fn effective_markdown_style(&self) -> RenderMarkdownStyle {
        self.markdown_style.into()
    }
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
            markdown_style: MarkdownStyleArg::Clean,
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandaloneOcrMode {
    Auto,
    Force,
}

impl From<StandaloneOcrMode> for OcrMode {
    fn from(mode: StandaloneOcrMode) -> Self {
        match mode {
            StandaloneOcrMode::Auto => OcrMode::Auto,
            StandaloneOcrMode::Force => OcrMode::Force,
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridPolicy {
    Auto,
    All,
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
            markdown_style: MarkdownStyleArg::Clean,
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
            MarkdownStyleArg::Clean
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
        options.markdown_style = MarkdownStyleArg::Review;

        assert_eq!(options.effective_figure_mode(), FigureMode::Embedded);
        assert_eq!(options.effective_table_mode(), TableMode::Layout);
        assert_eq!(options.effective_formula_mode(), FormulaMode::Auto);
        assert!(!options.effective_render_math());
    }

    #[test]
    fn clean_style_keeps_selected_table_mode_without_disabling_math_rendering() {
        let mut options = convert_options(false);
        options.markdown_style = MarkdownStyleArg::Clean;
        options.table_mode = TableMode::Layout;

        assert_eq!(options.effective_table_mode(), TableMode::Layout);
        assert!(options.effective_render_math());
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridMode {
    Off,
    Docling,
}

impl HybridMode {
    pub fn is_on(self) -> bool {
        !matches!(self, HybridMode::Off)
    }
}

/// All file extensions that pdfp supports.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["pdf"];

/// Check whether a file path has a supported PDF extension.
pub fn is_pdf(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}
