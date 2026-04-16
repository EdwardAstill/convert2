mod batch;
mod cli;
mod docx;
mod document;
mod epub;
mod error;
mod formats;
mod html_extract;
mod layout;
mod pdf;
mod pptx;
mod render;
mod svg;
mod typst;

use anyhow::Context;
use clap::Parser;

use cli::{Cli, Format, InputType};
use layout::{
    classifier::Classifier,
    xycut::{XyCutConfig, build_xycut_tree, assign_reading_order},
};
use pdf::extractor::PdfExtractor;
use render::markdown::MarkdownRenderer;
use document::types::{Document, Page};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let inputs = batch::resolve_inputs(&cli.input)
        .with_context(|| format!("Failed to resolve input '{}'", cli.input))?;

    if cli.verbose {
        eprintln!("Processing {} file(s)", inputs.len());
    }

    let results: Vec<(std::path::PathBuf, anyhow::Result<()>)> = inputs
        .iter()
        .map(|path| {
            let result = process_one(path, &cli);
            (path.clone(), result)
        })
        .collect();

    let mut had_errors = false;
    for (path, result) in &results {
        match result {
            Ok(()) => {
                if cli.verbose {
                    eprintln!("  ok: {}", path.display());
                }
            }
            Err(e) => {
                eprintln!("  error: {}: {}", path.display(), e);
                had_errors = true;
            }
        }
    }

    if had_errors {
        std::process::exit(1);
    }

    Ok(())
}

/// Route a single file to the appropriate conversion pipeline.
fn process_one(
    path: &std::path::Path,
    cli: &Cli,
) -> anyhow::Result<()> {
    let input_type = InputType::from_path(path)
        .ok_or_else(|| anyhow::anyhow!("Unsupported file type: {}", path.display()))?;

    let format = cli.format.clone().unwrap_or_else(|| input_type.default_format());

    if !input_type.supports_format(&format) {
        anyhow::bail!(
            "Format {:?} is not supported for {:?} files",
            format,
            input_type
        );
    }

    match input_type {
        InputType::Pdf => process_pdf(path, cli, &format),
        InputType::Docx => process_docx(path, cli, &format),
        InputType::Epub => process_epub(path, cli, &format),
        InputType::Pptx => process_pptx(path, cli, &format),
        InputType::Html => process_html(path, cli, &format),
        InputType::Markdown => process_md_to_typst(path, cli),
        InputType::Svg => process_svg_to_png(path, cli),
    }
}

/// Pipeline A: PDF → Document → Markdown → Format writer
fn process_pdf(
    pdf_path: &std::path::Path,
    cli: &Cli,
    format: &Format,
) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing PDF: {}", pdf_path.display());
    }

    let xycut_config = XyCutConfig {
        min_horizontal_gap: cli.min_h_gap,
        min_vertical_gap: cli.min_v_gap,
        ..Default::default()
    };

    // Extract raw pages and metadata in a single file open
    let (raw_pages, metadata) = PdfExtractor::extract(pdf_path)
        .with_context(|| format!("Failed to extract {}", pdf_path.display()))?;

    // Build classifier from document-level font size stats
    let classifier = Classifier::new_for_document(&raw_pages);

    // Process each page: XY-Cut → classify → Page
    let pages: Vec<Page> = raw_pages
        .into_iter()
        .map(|mut raw_page| {
            let tree = build_xycut_tree(&raw_page.blocks, &xycut_config);
            assign_reading_order(&tree, &mut raw_page.blocks);
            use document::types::RawPage;
            let page_shell = RawPage {
                page_num: raw_page.page_num,
                width: raw_page.width,
                height: raw_page.height,
                blocks: Vec::new(),
                image_refs: Vec::new(),
            };
            let blocks = classifier.classify_page(raw_page.blocks, &page_shell);
            Page {
                page_num: page_shell.page_num,
                width: page_shell.width,
                height: page_shell.height,
                blocks,
            }
        })
        .collect();

    // Warn about pages with no extractable text
    let empty_page_count = pages.iter().filter(|p| p.blocks.is_empty()).count();
    if empty_page_count > 0 {
        eprintln!(
            "  warning: {} of {} pages have no extractable text (possibly scanned)",
            empty_page_count,
            pages.len()
        );
    }

    let doc = Document {
        source_path: pdf_path.to_path_buf(),
        pages,
        metadata,
    };

    write_document(&doc, pdf_path, cli, format)
}

/// Shared: render Document to markdown and write via format writer.
fn write_document(
    doc: &Document,
    input_path: &std::path::Path,
    cli: &Cli,
    format: &Format,
) -> anyhow::Result<()> {
    let output_dir = batch::output_dir_for(input_path, cli.output.as_deref());
    let renderer = MarkdownRenderer::new(!cli.no_images, Some(output_dir.join("images")));
    let rendered = renderer.render_document(doc)
        .with_context(|| "Failed to render markdown")?;

    let stem = input_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    match format {
        Format::Raw => {
            formats::raw::RawFormat::write(&rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write output to {}", output_dir.display()))?;
        }
        Format::Rag => {
            formats::rag::RagFormat::new(cli.chunk_size)
                .write(&rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write RAG output to {}", output_dir.display()))?;
        }
        Format::Karpathy => {
            formats::karpathy::KarpathyFormat::write(&rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write Karpathy output to {}", output_dir.display()))?;
        }
        Format::Kg => {
            formats::kg::KgFormat::write(&rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write KG output to {}", output_dir.display()))?;
        }
        _ => unreachable!("format validated against input type"),
    }

    Ok(())
}

// --- Stubs for new pipelines (implemented in later phases) ---

/// Pipeline A: DOCX → Document → Markdown → Format writer
fn process_docx(
    path: &std::path::Path,
    cli: &Cli,
    format: &Format,
) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing DOCX: {}", path.display());
    }
    let doc = docx::extractor::extract(path)
        .with_context(|| format!("Failed to extract {}", path.display()))?;
    write_document(&doc, path, cli, format)
}

/// Pipeline A: EPUB → Document → Markdown → Format writer
fn process_epub(
    path: &std::path::Path,
    cli: &Cli,
    format: &Format,
) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing EPUB: {}", path.display());
    }
    let doc = epub::extractor::extract(path)
        .with_context(|| format!("Failed to extract {}", path.display()))?;
    write_document(&doc, path, cli, format)
}

/// Pipeline A: PPTX → Document → Markdown → Format writer
fn process_pptx(
    path: &std::path::Path,
    cli: &Cli,
    format: &Format,
) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing PPTX: {}", path.display());
    }
    let doc = pptx::extractor::extract(path)
        .with_context(|| format!("Failed to extract {}", path.display()))?;
    write_document(&doc, path, cli, format)
}

/// Pipeline A: HTML → Document → Markdown → Format writer
fn process_html(
    path: &std::path::Path,
    cli: &Cli,
    format: &Format,
) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing HTML: {}", path.display());
    }
    let doc = html_extract::extractor::extract(path)
        .with_context(|| format!("Failed to extract {}", path.display()))?;
    write_document(&doc, path, cli, format)
}

fn process_md_to_typst(
    path: &std::path::Path,
    cli: &Cli,
) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing Markdown → Typst: {}", path.display());
    }

    let md_text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let config = typst::config::load_config(cli.typst_config.as_deref());

    // CLI --paper overrides config
    let paper = cli.paper.as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let p = &config.page.paper;
            if p.is_empty() { None } else { Some(p.as_str()) }
        });

    let mut result = typst::converter::convert(&md_text, &config);

    if let Some(paper) = paper {
        result = format!("#set page(paper: \"{paper}\")\n\n{result}");
    }

    if cli.stdout {
        print!("{result}");
    } else {
        let output_path = cli.output.as_ref()
            .map(|o| o.to_path_buf())
            .unwrap_or_else(|| path.with_extension("typ"));
        std::fs::write(&output_path, &result)
            .with_context(|| format!("Failed to write {}", output_path.display()))?;
        if cli.verbose {
            eprintln!("  wrote: {}", output_path.display());
        }
    }

    Ok(())
}

/// Pipeline C: SVG → PNG
fn process_svg_to_png(
    path: &std::path::Path,
    cli: &Cli,
) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing SVG → PNG: {}", path.display());
    }
    let output_path = cli.output.as_ref()
        .map(|o| o.to_path_buf())
        .unwrap_or_else(|| path.with_extension("png"));
    svg::converter::convert(path, &output_path)
        .with_context(|| format!("Failed to convert {}", path.display()))?;
    if cli.verbose {
        eprintln!("  wrote: {}", output_path.display());
    }
    Ok(())
}
