mod batch;
mod cli;
mod docx;
mod document;
mod epub;
mod error;
mod formats;
mod html_extract;
mod hybrid;
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
    xycut::{XyCutConfig, assign_reading_order, build_xycut_order},
};
use pdf::extractor::PdfExtractor;
use render::markdown::MarkdownRenderer;
use document::types::{Block, BlockKind, Document, ImageRef, Page};

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

    // Images are saved to `<output_dir>/images/` and referenced from the
    // markdown via the relative path `images/<file>`. The output dir is also
    // re-derived in write_document() — `batch::output_dir_for` is pure.
    let output_dir = batch::output_dir_for(pdf_path, cli.output.as_deref());
    let images_dir = output_dir.join("images");
    let extract_images = !cli.no_images;

    // Process each page: XY-Cut++ on text → classify → save image bytes → merge.
    let pages: Vec<Page> = raw_pages
        .into_iter()
        .map(|raw_page| -> anyhow::Result<Page> {
            let mut text_blocks = raw_page.blocks;
            let order = build_xycut_order(&text_blocks, &xycut_config);
            assign_reading_order(&order, &mut text_blocks);

            use document::types::RawPage;
            let page_shell = RawPage {
                page_num: raw_page.page_num,
                width: raw_page.width,
                height: raw_page.height,
                blocks: Vec::new(),
                image_refs: Vec::new(),
            };
            let metadata = pdf::metadata::load_page_metadata(pdf_path, raw_page.page_num);
            let text_classified = classifier.classify_page_with_metadata(
                text_blocks,
                &page_shell,
                metadata.as_ref(),
            );

            let image_blocks = if extract_images && !raw_page.image_refs.is_empty() {
                save_page_images(&raw_page.image_refs, &images_dir)?
            } else {
                Vec::new()
            };

            let merged = merge_text_and_images(text_classified, image_blocks);

            Ok(Page {
                page_num: raw_page.page_num,
                width: raw_page.width,
                height: raw_page.height,
                blocks: merged,
                override_markdown: None,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Warn about pages with no extractable text
    let empty_page_count = pages.iter().filter(|p| p.blocks.is_empty()).count();
    if empty_page_count > 0 {
        eprintln!(
            "  warning: {} of {} pages have no extractable text (possibly scanned)",
            empty_page_count,
            pages.len()
        );
    }

    let mut doc = Document {
        source_path: pdf_path.to_path_buf(),
        pages,
        metadata,
    };

    // Hybrid pass: triage-qualifying pages are uploaded to the external
    // backend; their markdown replaces the local rendering in the output.
    if cli.hybrid.is_on() {
        let policy = match cli.hybrid_policy {
            cli::HybridPolicy::Auto => hybrid::RoutingPolicy::Auto,
            cli::HybridPolicy::All => hybrid::RoutingPolicy::All,
        };
        let timeout = std::time::Duration::from_secs(cli.hybrid_timeout_secs);
        let stats = hybrid::apply_to_document(
            &mut doc,
            pdf_path,
            policy,
            &cli.hybrid_url,
            timeout,
            cli.verbose,
        )
        .with_context(|| {
            format!(
                "hybrid backend ({}) failed for {}",
                cli.hybrid_url,
                pdf_path.display()
            )
        })?;
        if cli.verbose {
            eprintln!(
                "  hybrid: routed {}/{} pages ({} failed)",
                stats.pages_routed, stats.pages_total, stats.pages_failed
            );
        }
    }

    write_document(&doc, pdf_path, cli, format)
}

/// Write each `ImageRef`'s PNG bytes to `images_dir` and produce a matching
/// `Block` with `BlockKind::Image` whose path is relative to the output dir.
fn save_page_images(
    image_refs: &[ImageRef],
    images_dir: &std::path::Path,
) -> anyhow::Result<Vec<Block>> {
    std::fs::create_dir_all(images_dir)
        .with_context(|| format!("Failed to create images dir {}", images_dir.display()))?;

    let mut blocks: Vec<Block> = Vec::with_capacity(image_refs.len());
    for img_ref in image_refs {
        let filename = format!(
            "page{}_img{}.{}",
            img_ref.page_num + 1,
            img_ref.image_index + 1,
            img_ref.format,
        );
        let abs_path = images_dir.join(&filename);
        std::fs::write(&abs_path, &img_ref.bytes)
            .with_context(|| format!("Failed to write image {}", abs_path.display()))?;
        let rel_path = format!("images/{filename}");
        blocks.push(Block {
            // Use a high id to avoid collision with text-block ids.
            id: 1_000_000 + img_ref.image_index,
            bbox: img_ref.bbox,
            text: String::new(),
            kind: BlockKind::Image { path: Some(rel_path) },
            font_size: 0.0,
            font_name: "image".to_string(),
            page_num: img_ref.page_num,
            reading_order: 0,
        });
    }
    Ok(blocks)
}

/// Interleave image blocks among text blocks by Y position.
///
/// The text blocks already carry XY-Cut++ reading order. Images are inserted
/// in front of the first text block whose `y0` is greater than the image's
/// `y0`, then `reading_order` is re-assigned across the merged sequence. This
/// is a rough approximation — on multi-column pages an image's true anchor is
/// in a specific column, but for markdown output it is fine to place it in
/// the natural Y slot.
fn merge_text_and_images(mut text: Vec<Block>, mut images: Vec<Block>) -> Vec<Block> {
    if images.is_empty() {
        return text;
    }
    text.sort_by_key(|b| b.reading_order);
    images.sort_by(|a, b| {
        a.bbox
            .y0
            .partial_cmp(&b.bbox.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut result: Vec<Block> = Vec::with_capacity(text.len() + images.len());
    let mut img_iter = images.into_iter().peekable();
    let mut order: usize = 0;
    for mut tb in text {
        while let Some(peek) = img_iter.peek() {
            if peek.bbox.y0 < tb.bbox.y0 {
                let mut img = img_iter.next().expect("peek succeeded");
                img.reading_order = order;
                order += 1;
                result.push(img);
            } else {
                break;
            }
        }
        tb.reading_order = order;
        order += 1;
        result.push(tb);
    }
    for mut img in img_iter {
        img.reading_order = order;
        order += 1;
        result.push(img);
    }
    result
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
    write_rendered(&rendered, doc, input_path, cli, format)
}

/// Shared: write an already-rendered document via the chosen format writer.
fn write_rendered(
    rendered: &render::markdown::RenderedDocument,
    doc: &Document,
    input_path: &std::path::Path,
    cli: &Cli,
    format: &Format,
) -> anyhow::Result<()> {
    let output_dir = batch::output_dir_for(input_path, cli.output.as_deref());
    let stem = input_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    match format {
        Format::Raw => {
            formats::raw::RawFormat::write(rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write output to {}", output_dir.display()))?;
        }
        Format::Rag => {
            formats::rag::RagFormat::new(cli.chunk_size)
                .write(rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write RAG output to {}", output_dir.display()))?;
        }
        Format::Karpathy => {
            formats::karpathy::KarpathyFormat::write(rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write Karpathy output to {}", output_dir.display()))?;
        }
        Format::Kg => {
            formats::kg::KgFormat::write(rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write KG output to {}", output_dir.display()))?;
        }
        Format::Json => {
            formats::json::JsonFormat::write(rendered, doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write JSON output to {}", output_dir.display()))?;
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
