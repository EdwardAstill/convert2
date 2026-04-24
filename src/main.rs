mod batch;
mod cli;
mod document;
mod error;
mod formats;
mod hybrid;
mod layout;
mod pdf;
mod render;

use anyhow::Context;
use clap::Parser;

use cli::{Cli, InputType};
use document::types::{Block, BlockKind, Document, ImageRef, Page};
use layout::{
    classifier::Classifier,
    xycut::{assign_reading_order, build_xycut_order, XyCutConfig},
};
use pdf::extractor::PdfExtractor;
use render::markdown::MarkdownRenderer;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let inputs = batch::resolve_inputs(&cli.input)
        .with_context(|| format!("Failed to resolve input '{}'", cli.input))?;

    if cli.verbose {
        eprintln!("Processing {} PDF file(s)", inputs.len());
    }

    let results: Vec<(std::path::PathBuf, anyhow::Result<()>)> = inputs
        .iter()
        .map(|path| (path.clone(), process_one(path, &cli)))
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

fn process_one(path: &std::path::Path, cli: &Cli) -> anyhow::Result<()> {
    let input_type = InputType::from_path(path)
        .ok_or_else(|| anyhow::anyhow!("Unsupported file type: {}", path.display()))?;

    match input_type {
        InputType::Pdf => process_pdf(path, cli),
    }
}

fn process_pdf(pdf_path: &std::path::Path, cli: &Cli) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing PDF: {}", pdf_path.display());
    }

    let xycut_config = XyCutConfig {
        min_horizontal_gap: cli.min_h_gap,
        min_vertical_gap: cli.min_v_gap,
        ..Default::default()
    };

    let (raw_pages, metadata) = PdfExtractor::extract(pdf_path)
        .with_context(|| format!("Failed to extract {}", pdf_path.display()))?;

    let classifier = Classifier::new_for_document(&raw_pages);

    let output_dir = batch::output_dir_for(pdf_path, cli.output.as_deref());
    let images_dir = output_dir.join("images");
    let extract_images = !cli.no_images;

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
            let text_classified =
                classifier.classify_page_with_metadata(text_blocks, &page_shell, metadata.as_ref());

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

    let empty_page_count = pages.iter().filter(|p| p.blocks.is_empty()).count();
    if empty_page_count > 0 {
        eprintln!(
            "  warning: {} of {} pages have no extractable text (possibly scanned)",
            empty_page_count,
            pages.len()
        );
    }

    let scan_report = hybrid::triage::scan_report(&pages);
    if !cli.hybrid.is_on() && scan_report.likely_scan_like() {
        eprintln!(
            "  warning: {} looks scan-heavy ({} image-only / {} low-density page(s), {} page(s) with readable text); local output may be poor. Try `--hybrid docling`.",
            pdf_path.display(),
            scan_report.image_only_pages,
            scan_report.low_density_pages,
            scan_report.pages_with_readable_text
        );
    }

    let mut doc = Document {
        source_path: pdf_path.to_path_buf(),
        pages,
        metadata,
    };

    if cli.hybrid.is_on() {
        let policy = match cli.hybrid_policy {
            cli::HybridPolicy::Auto if scan_report.likely_scan_like() => {
                if cli.verbose {
                    eprintln!(
                        "  hybrid: document looks scan-heavy; upgrading auto policy to route all pages"
                    );
                }
                hybrid::RoutingPolicy::All
            }
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

    write_document(&doc, pdf_path, cli)
}

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
            id: 1_000_000 + img_ref.image_index,
            bbox: img_ref.bbox,
            text: String::new(),
            kind: BlockKind::Image {
                path: Some(rel_path),
            },
            font_size: 0.0,
            font_name: "image".to_string(),
            page_num: img_ref.page_num,
            reading_order: 0,
        });
    }
    Ok(blocks)
}

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

fn write_document(doc: &Document, input_path: &std::path::Path, cli: &Cli) -> anyhow::Result<()> {
    let output_dir = batch::output_dir_for(input_path, cli.output.as_deref());
    let renderer = MarkdownRenderer::new(!cli.no_images, Some(output_dir.join("images")));
    let rendered = renderer
        .render_document(doc)
        .with_context(|| "Failed to render markdown")?;

    let stem = input_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    formats::raw::RawFormat::write(&rendered, doc, &output_dir, &stem)
        .with_context(|| format!("Failed to write output to {}", output_dir.display()))
}
