mod batch;
mod cli;
mod document;
mod error;
mod formats;
mod layout;
mod pdf;
mod render;

use anyhow::Context;
use clap::Parser;
use rayon::prelude::*;

use cli::{Cli, Format};
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
        eprintln!("Processing {} PDF(s)", inputs.len());
    }

    let xycut_config = XyCutConfig {
        min_horizontal_gap: cli.min_h_gap,
        min_vertical_gap: cli.min_v_gap,
        ..Default::default()
    };

    let results: Vec<(std::path::PathBuf, anyhow::Result<()>)> = inputs
        .par_iter()
        .map(|pdf_path| {
            let result = process_one(pdf_path, &cli, &xycut_config);
            (pdf_path.clone(), result)
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

fn process_one(
    pdf_path: &std::path::Path,
    cli: &Cli,
    xycut_config: &XyCutConfig,
) -> anyhow::Result<()> {
    if cli.verbose {
        eprintln!("  processing: {}", pdf_path.display());
    }

    // Extract raw pages
    let raw_pages = PdfExtractor::extract_pages(pdf_path)
        .with_context(|| format!("Failed to extract pages from {}", pdf_path.display()))?;

    let metadata = PdfExtractor::extract_metadata(pdf_path)
        .with_context(|| format!("Failed to extract metadata from {}", pdf_path.display()))?;

    // Build classifier from document-level font size stats
    let classifier = Classifier::new_for_document(&raw_pages);

    // Process each page: XY-Cut → classify → Page
    let pages: Vec<Page> = raw_pages
        .into_iter()
        .map(|mut raw_page| {
            let tree = build_xycut_tree(&raw_page.blocks, xycut_config);
            assign_reading_order(&tree, &mut raw_page.blocks);
            // Destructure to avoid partial-move borrow conflict:
            // classify_page needs &RawPage for dimensions but takes Vec<RawTextBlock> by value.
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

    let doc = Document {
        source_path: pdf_path.to_path_buf(),
        pages,
        metadata,
    };

    // Render to markdown
    let output_dir = batch::output_dir_for(pdf_path, cli.output.as_deref());
    let renderer = MarkdownRenderer::new(!cli.no_images, Some(output_dir.join("images")));
    let rendered = renderer.render_document(&doc)
        .with_context(|| "Failed to render markdown")?;

    let stem = pdf_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Write output
    match cli.format {
        Format::Raw => {
            formats::raw::RawFormat::write(&rendered, &doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write output to {}", output_dir.display()))?;
        }
        Format::Rag | Format::Karpathy | Format::Kg => {
            // Stub — not yet implemented
            eprintln!("Format {:?} not yet implemented, falling back to raw", cli.format);
            formats::raw::RawFormat::write(&rendered, &doc, &output_dir, &stem)
                .with_context(|| format!("Failed to write output to {}", output_dir.display()))?;
        }
    }

    Ok(())
}
