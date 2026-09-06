mod formulas;
mod media;
mod merge;
mod routing;
mod tables;

use formulas::*;
use media::*;
use routing::*;
use tables::*;

use std::path::Path;
use std::time::Duration;

use anyhow::Context;

use crate::batch;
use crate::config::{self, ConvertOptions, FigureMode, FormulaMode, TableMode};
use crate::document::types::{Bbox, Document, Page, RawPage};
use crate::error::PdfpResult;
use crate::figure::{detect_figure_candidates, render_figure_snapshots, FigureDetectionOptions};
use crate::formats;
use crate::formula::ocr::FormulaSidecar;
use crate::formula::{detect_formula_candidates, detect_visual_formula_candidates};
use crate::hybrid;
use crate::layout::{
    classifier::Classifier,
    furniture::detect_furniture_bboxes,
    table::{detect_coordinate_tables, TableCandidate},
    xycut::{assign_reading_order, build_xycut_order, XyCutConfig},
};
use crate::ocr;
use crate::pdf::{self, extractor::PdfExtractor};
use crate::processor::page_range::parse_page_selection;
use crate::render::markdown::MarkdownRenderer;

use merge::{
    formula_excluded_regions, merge_media_blocks, merge_text_and_formulas, merge_text_and_images,
    merge_text_and_tables, suppress_formula_candidates_overlapping_tables,
    suppress_overlapping_table_candidates, suppress_text_covered_by_formulas,
    suppress_text_covered_by_furniture, suppress_text_covered_by_tables,
};

pub fn process_pdf(pdf_path: &Path, options: &ConvertOptions) -> PdfpResult<()> {
    if options.verbose {
        eprintln!("  processing PDF: {}", pdf_path.display());
    }

    let doc = process_pdf_to_document(pdf_path, options)?;
    Ok(write_document(&doc, pdf_path, options)?)
}

pub fn process_pdf_to_document(pdf_path: &Path, options: &ConvertOptions) -> PdfpResult<Document> {
    let xycut_config = XyCutConfig {
        min_horizontal_gap: options.min_h_gap,
        min_vertical_gap: options.min_v_gap,
        ..Default::default()
    };

    let (raw_pages, metadata) = PdfExtractor::extract(pdf_path)
        .with_context(|| format!("Failed to extract {}", pdf_path.display()))?;
    let raw_pages = select_raw_pages(raw_pages, options)?;
    let ocr_report = ocr::triage::triage_raw_pages(&raw_pages);
    let prepared =
        ocr::prepare_pdf_with_report(pdf_path, &options.ocr, &ocr_report, options.verbose)?;

    let mut doc = if prepared.effective_path == pdf_path {
        build_document_from_raw(
            pdf_path,
            pdf_path,
            raw_pages,
            metadata,
            options,
            &xycut_config,
        )?
    } else {
        build_document(&prepared.effective_path, pdf_path, options, &xycut_config)?
    };
    let scan_report = hybrid::triage::scan_report(&doc.pages);

    warn_on_scan_like_pages(pdf_path, options, &doc.pages, &scan_report);
    apply_hybrid_if_enabled(&mut doc, pdf_path, options, &scan_report)?;
    Ok(doc)
}

fn build_document(
    pdf_path: &Path,
    output_base_path: &Path,
    options: &ConvertOptions,
    xycut_config: &XyCutConfig,
) -> anyhow::Result<Document> {
    let (raw_pages, metadata) = PdfExtractor::extract(pdf_path)
        .with_context(|| format!("Failed to extract {}", pdf_path.display()))?;
    let raw_pages = select_raw_pages(raw_pages, options)?;
    build_document_from_raw(
        pdf_path,
        output_base_path,
        raw_pages,
        metadata,
        options,
        xycut_config,
    )
}

fn select_raw_pages(
    raw_pages: Vec<RawPage>,
    options: &ConvertOptions,
) -> anyhow::Result<Vec<RawPage>> {
    let Some(spec) = options.pages.as_deref() else {
        return Ok(raw_pages);
    };
    let selected: std::collections::BTreeSet<usize> = parse_page_selection(spec, raw_pages.len())?
        .into_iter()
        .collect();
    Ok(raw_pages
        .into_iter()
        .filter(|page| selected.contains(&page.page_num))
        .collect())
}

fn create_requested_asset_dirs(
    output_dir: &Path,
    options: &config::ConvertOptions,
) -> anyhow::Result<()> {
    for dir in [
        options.effective_image_output().then_some("images"),
        options.export_table_images().then_some("tables"),
        options.export_equation_images().then_some("equations"),
    ]
    .into_iter()
    .flatten()
    {
        let path = output_dir.join(dir);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create asset dir {}", path.display()))?;
    }
    Ok(())
}

fn build_document_from_raw(
    pdf_path: &Path,
    output_base_path: &Path,
    raw_pages: Vec<RawPage>,
    metadata: crate::document::types::DocumentMetadata,
    options: &ConvertOptions,
    xycut_config: &XyCutConfig,
) -> anyhow::Result<Document> {
    let classifier = Classifier::new_for_document(&raw_pages);
    let furniture_mask = detect_furniture_bboxes(&raw_pages);
    let formula_sidecar = build_formula_sidecar(
        options.formula_sidecar.as_deref(),
        Duration::from_secs(options.formula_sidecar_timeout_secs),
    )?;
    let table_geometry_doc = mupdf::Document::open(pdf_path).ok();
    let output_dir = batch::conversion_output_dir_for(
        output_base_path,
        options.output.as_deref(),
        options.batch_mode,
    );
    let images_dir = output_dir.join("images");
    create_requested_asset_dirs(&output_dir, options)?;
    let figure_mode = options.effective_figure_mode();
    let image_output_enabled = options.effective_image_output();
    let extract_embedded_images =
        image_output_enabled && matches!(figure_mode, FigureMode::Embedded | FigureMode::Both);
    let extract_snapshot_figures =
        image_output_enabled && matches!(figure_mode, FigureMode::Snapshot | FigureMode::Both);

    let page_build_context = PageBuildContext {
        pdf_path,
        classifier: &classifier,
        xycut_config,
        extract_embedded_images,
        extract_snapshot_figures,
        images_dir: &images_dir,
        output_dir: &output_dir,
        options,
        furniture_mask: &furniture_mask,
        formula_sidecar: formula_sidecar.as_deref(),
        table_geometry_doc: table_geometry_doc.as_ref(),
    };

    let mut pages = Vec::new();
    let mut formula_records = Vec::new();
    let mut formula_candidate_pages = 0usize;
    let mut formula_candidate_count = 0usize;
    for raw_page in raw_pages {
        let built = build_page(raw_page, &page_build_context)?;
        if built.formula_candidate_count > 0 {
            formula_candidate_pages += 1;
            formula_candidate_count += built.formula_candidate_count;
        }
        formula_records.extend(built.formula_records);
        pages.push(built.page);
    }
    warn_on_formula_candidate_summary(options, formula_candidate_pages, formula_candidate_count);
    if options.debug_formulas || options.formula_sidecar.is_some() {
        write_formula_index(&output_dir, output_base_path, pages.len(), &formula_records)?;
    }

    Ok(Document {
        source_path: pdf_path.to_path_buf(),
        pages,
        metadata,
    })
}

struct PageBuildContext<'a> {
    pdf_path: &'a Path,
    classifier: &'a Classifier,
    xycut_config: &'a XyCutConfig,
    extract_embedded_images: bool,
    extract_snapshot_figures: bool,
    images_dir: &'a Path,
    output_dir: &'a Path,
    options: &'a ConvertOptions,
    furniture_mask: &'a std::collections::HashMap<usize, Vec<Bbox>>,
    formula_sidecar: Option<&'a dyn FormulaSidecar>,
    table_geometry_doc: Option<&'a mupdf::Document>,
}

struct BuiltPage {
    page: Page,
    formula_candidate_count: usize,
    formula_records: Vec<FormulaReportRecord>,
}

fn build_page(mut raw_page: RawPage, ctx: &PageBuildContext<'_>) -> anyhow::Result<BuiltPage> {
    let mut text_blocks = std::mem::take(&mut raw_page.blocks);
    let order = build_xycut_order(&text_blocks, ctx.xycut_config);
    assign_reading_order(&order, &mut text_blocks);

    let page_shell = RawPage {
        page_num: raw_page.page_num,
        width: raw_page.width,
        height: raw_page.height,
        blocks: Vec::new(),
        words: Vec::new(),
        image_refs: Vec::new(),
    };
    let table_mode = ctx.options.effective_table_mode();
    let formula_mode = ctx.options.effective_formula_mode();
    let mut table_candidates =
        detect_coordinate_tables(&raw_page.words, raw_page.width, table_mode);
    table_candidates.extend(detect_geometry_table_candidates(
        ctx.table_geometry_doc,
        &raw_page,
        table_mode,
    ));
    let table_candidates = suppress_overlapping_table_candidates(table_candidates);
    let table_candidates: Vec<TableCandidate> = table_candidates
        .into_iter()
        .filter(|candidate| candidate.should_emit(raw_page.width, raw_page.height))
        .collect();
    let furniture_bboxes = ctx
        .furniture_mask
        .get(&raw_page.page_num)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let formula_blocking_tables: Vec<TableCandidate> = table_candidates
        .iter()
        .filter(|candidate| {
            candidate.table.confidence >= 0.70
                && !is_broad_layout_table_candidate(candidate, raw_page.height)
        })
        .cloned()
        .collect();
    let excluded_regions = formula_excluded_regions(&formula_blocking_tables, furniture_bboxes);
    if ctx.options.debug_tables && !matches!(table_mode, TableMode::Off) {
        write_table_debug(ctx.output_dir, raw_page.page_num, &table_candidates)?;
    }
    if ctx.options.export_table_images() && !matches!(table_mode, TableMode::Off) {
        write_table_crops(
            ctx.pdf_path,
            ctx.output_dir,
            raw_page.page_num,
            &table_candidates,
            ctx.options.figure_dpi,
        )?;
    }
    let mut formula_candidates = if matches!(formula_mode, FormulaMode::Off) {
        Vec::new()
    } else {
        detect_formula_candidates(&raw_page, &excluded_regions)
    };
    formula_candidates = suppress_formula_candidates_overlapping_tables(
        formula_candidates,
        &formula_blocking_tables,
    );
    if (ctx.options.debug_formulas || ctx.options.export_equation_images())
        && !matches!(formula_mode, FormulaMode::Off)
    {
        let visual_candidates = detect_visual_formula_candidates(
            ctx.pdf_path,
            &raw_page,
            &formula_candidates,
            &excluded_regions,
        )?;
        formula_candidates.extend(visual_candidates);
        renumber_formula_candidates(&mut formula_candidates);
    }
    if (ctx.options.debug_formulas
        || ctx.formula_sidecar.is_some()
        || ctx.options.export_equation_images())
        && !matches!(formula_mode, FormulaMode::Off)
    {
        let (crop_dir, crop_rel_dir) = if ctx.options.export_equation_images() {
            (ctx.output_dir.join("equations"), "equations")
        } else {
            (
                ctx.output_dir.join("debug").join("formulas"),
                "debug/formulas",
            )
        };
        write_formula_debug(FormulaDebugParams {
            pdf_path: ctx.pdf_path,
            crop_dir: &crop_dir,
            crop_rel_dir,
            page_num: raw_page.page_num,
            candidates: &mut formula_candidates,
            dpi: ctx.options.figure_dpi,
            sidecar: ctx.formula_sidecar,
            write_json: ctx.options.debug_formulas,
        })?;
    }
    let metadata = pdf::metadata::load_page_metadata(ctx.pdf_path, raw_page.page_num);
    let text_classified =
        ctx.classifier
            .classify_page_with_metadata(text_blocks, &page_shell, metadata.as_ref());
    let text_classified = suppress_text_covered_by_furniture(text_classified, furniture_bboxes);
    let text_classified = suppress_text_covered_by_tables(text_classified, &table_candidates);
    let formula_candidate_count = formula_candidates.len();
    let formula_records = formula_report_records(
        &formula_candidates,
        formula_mode,
        ctx.options.effective_render_math(),
        ctx.options.formula_emit,
    );
    let table_blocks = table_candidates_to_blocks(raw_page.page_num, table_candidates);
    let formula_blocks = formula_candidates_to_blocks(
        raw_page.page_num,
        formula_candidates,
        formula_mode,
        ctx.options.effective_render_math(),
        ctx.options.formula_emit,
    );
    let text_classified = suppress_text_covered_by_formulas(text_classified, &formula_blocks);

    let figure_candidates = if ctx.extract_snapshot_figures {
        detect_figure_candidates(
            &raw_page,
            &text_classified,
            FigureDetectionOptions {
                padding: ctx.options.figure_padding,
                ..Default::default()
            },
        )
    } else {
        Vec::new()
    };

    if ctx.options.debug_figures && ctx.extract_snapshot_figures {
        write_figure_debug(ctx.output_dir, raw_page.page_num, &figure_candidates)?;
    }

    let image_blocks = if ctx.extract_embedded_images && !raw_page.image_refs.is_empty() {
        save_page_images(&raw_page.image_refs, ctx.images_dir)?
    } else {
        Vec::new()
    };
    let figure_blocks = if ctx.extract_snapshot_figures && !figure_candidates.is_empty() {
        render_figure_snapshots(
            ctx.pdf_path,
            raw_page.page_num,
            &figure_candidates,
            ctx.images_dir,
            ctx.options.figure_dpi,
        )?
        .into_iter()
        .map(|rendered| rendered.block)
        .collect()
    } else {
        Vec::new()
    };

    Ok(BuiltPage {
        page: Page {
            page_num: raw_page.page_num,
            width: raw_page.width,
            height: raw_page.height,
            blocks: merge_text_and_images(
                merge_text_and_formulas(
                    merge_text_and_tables(text_classified, table_blocks),
                    formula_blocks,
                ),
                merge_media_blocks(image_blocks, figure_blocks),
            ),
            override_markdown: None,
        },
        formula_candidate_count,
        formula_records,
    })
}

fn write_document(
    doc: &Document,
    input_path: &Path,
    options: &ConvertOptions,
) -> anyhow::Result<()> {
    let output_dir =
        batch::conversion_output_dir_for(input_path, options.output.as_deref(), options.batch_mode);
    let renderer = MarkdownRenderer::with_style(
        options.effective_image_output(),
        Some(output_dir.join("images")),
        options.effective_markdown_style(),
    );
    let rendered = renderer
        .render_document(doc)
        .with_context(|| "Failed to render markdown")?;

    let stem = input_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    formats::RawFormat::write(&rendered, doc, &output_dir, &stem)
        .with_context(|| format!("Failed to write output to {}", output_dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FormulaEmitMode;
    use crate::document::types::BlockKind;
    use crate::formula::detect::{FormulaCandidate, FormulaStatus};
    use crate::formula::ocr::FormulaSidecarAttempt;

    fn formula_candidate(source_text: &str, confidence: u8) -> FormulaCandidate {
        FormulaCandidate {
            page_num: 0,
            formula_index: 0,
            bbox: Bbox::new(120.0, 140.0, 500.0, 162.0),
            source_text: source_text.into(),
            equation_number: None,
            confidence,
            status: FormulaStatus::LocalCandidate,
            backend: None,
            latex: None,
            words: Vec::new(),
            sidecar: FormulaSidecarAttempt::not_attempted(),
            reason: "test".into(),
            crop_path: None,
        }
    }

    #[test]
    fn auto_mode_promotes_only_high_confidence_formula_candidates() {
        let high = formula_candidate("E = mc^2", 70);
        let low = formula_candidate("a + b", 69);

        let blocks = formula_candidates_to_blocks(
            0,
            vec![high, low],
            FormulaMode::Auto,
            true,
            FormulaEmitMode::Auto,
        );

        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].kind, BlockKind::Formula { .. }));
        assert_eq!(blocks[0].text, "E = mc^2");
    }

    #[test]
    fn auto_policy_rejects_replacement_character_formula_text() {
        let candidate = formula_candidate("E = � + ???", 90);

        let blocks = formula_candidates_to_blocks(
            0,
            vec![candidate],
            FormulaMode::Auto,
            true,
            FormulaEmitMode::Auto,
        );

        assert!(
            blocks.is_empty(),
            "unsafe local formula text should not emit"
        );
    }

    #[test]
    fn auto_policy_rejects_malformed_matrix_like_fragments() {
        let candidate = formula_candidate("(1 2 4)(𝑥 𝑦) = ( 11) 5", 90);

        let blocks = formula_candidates_to_blocks(
            0,
            vec![candidate],
            FormulaMode::Auto,
            true,
            FormulaEmitMode::Auto,
        );

        assert!(
            blocks.is_empty(),
            "malformed matrix fragments should not emit"
        );
    }

    #[test]
    fn conservative_policy_rejects_local_heuristic_formula_text() {
        let candidate = formula_candidate("E = mc^2", 90);

        let blocks = formula_candidates_to_blocks(
            0,
            vec![candidate],
            FormulaMode::Auto,
            true,
            FormulaEmitMode::Conservative,
        );

        assert!(
            blocks.is_empty(),
            "conservative policy requires recovered LaTeX"
        );
    }

    #[test]
    fn sidecar_policy_skips_prose_like_high_confidence_candidates() {
        let candidate = formula_candidate(
            "We used the Adam optimizer with beta1 = 0.9 and beta2 = 0.98 during training",
            85,
        );

        assert!(!should_send_to_formula_sidecar(&candidate));
        assert_eq!(
            formula_sidecar_rejection_reason(&candidate),
            Some("candidate has too many words for sidecar OCR")
        );
    }

    #[test]
    fn sidecar_policy_skips_variable_definition_lines() {
        let candidate = formula_candidate(
            "γWeight = Unweighed object weight margin factor as per Table 5-2",
            77,
        );

        assert!(!should_send_to_formula_sidecar(&candidate));
        assert_eq!(
            formula_sidecar_rejection_reason(&candidate),
            Some("candidate looks like a variable definition line")
        );
    }

    #[test]
    fn sidecar_policy_skips_standards_table_and_prose_lines() {
        let table = formula_candidate(
            "Delta plates, master links and shackles <5 years < 12 months",
            73,
        );
        let prose = formula_candidate("h/λ > 0.3 Linear wave theory (or Stokes 5th order)", 77);
        let range = formula_candidate("1000t ≤ W 5000t ≤ W 20000t ≤", 77);

        assert!(!should_send_to_formula_sidecar(&table));
        assert_eq!(
            formula_sidecar_rejection_reason(&table),
            Some("candidate looks like a table/range comparison")
        );
        assert!(!should_send_to_formula_sidecar(&prose));
        assert_eq!(
            formula_sidecar_rejection_reason(&prose),
            Some("candidate looks like standards table/prose content")
        );
        assert!(!should_send_to_formula_sidecar(&range));
        assert_eq!(
            formula_sidecar_rejection_reason(&range),
            Some("candidate looks like a table/range comparison")
        );
    }

    #[test]
    fn sidecar_policy_keeps_compact_standards_formulas() {
        let weight = formula_candidate("WReport, Factored ≤ Wud/γWeight", 87);
        let padeye = formula_candidate("Rpad= (Rpl × tpl+2 × Rch × tch)/t", 85);

        assert!(should_send_to_formula_sidecar(&weight));
        assert!(should_send_to_formula_sidecar(&padeye));
    }

    #[test]
    fn sidecar_policy_keeps_numbered_and_reasonable_visual_formula_candidates() {
        let mut numbered = formula_candidate("E = mc^2 (1)", 96);
        numbered.equation_number = Some("(1)".into());
        let mut visual = formula_candidate("", 68);
        visual.bbox = Bbox::new(120.0, 140.0, 420.0, 162.0);
        visual.backend = Some("visual-page-render".into());
        visual.reason = "visual-isolated-equation-band+centered".into();

        assert!(should_send_to_formula_sidecar(&numbered));
        assert!(should_send_to_formula_sidecar(&visual));
    }

    #[test]
    fn sidecar_policy_rejects_wide_visual_formula_candidates() {
        let mut visual = formula_candidate("", 68);
        visual.bbox = Bbox::new(20.0, 140.0, 510.0, 170.0);
        visual.backend = Some("visual-page-render".into());
        visual.reason = "visual-isolated-equation-band+centered+horizontal-rule".into();

        assert!(!should_send_to_formula_sidecar(&visual));
        assert_eq!(
            formula_sidecar_rejection_reason(&visual),
            Some("visual-only crop too wide or ambiguous for sidecar OCR")
        );
    }

    #[test]
    fn formula_latex_strips_equation_number_and_adds_tag() {
        let mut candidate = formula_candidate("E = mc^2 (12)", 88);
        candidate.equation_number = Some("(12)".into());

        assert_eq!(build_formula_latex(&candidate), "E = mc^2 \\tag{12}");
    }

    #[test]
    fn formula_latex_normalizes_unicode_including_theta() {
        let candidate = formula_candidate("σ = √x + θ − Δ", 88);

        assert_eq!(
            build_formula_latex(&candidate),
            "\\sigma  = \\sqrt{} x + \\theta  - \\Delta "
        );
    }

    #[test]
    fn visual_only_candidate_becomes_review_block_without_math_rendering() {
        let mut candidate = formula_candidate("", 68);
        candidate.backend = Some("visual-page-render".into());
        candidate.reason = "visual-isolated-equation-band+cue:Hence:".into();
        candidate.crop_path = Some("debug/formulas/page1_formula1.png".into());

        let blocks = formula_candidates_to_blocks(
            0,
            vec![candidate],
            FormulaMode::Auto,
            false,
            FormulaEmitMode::Auto,
        );

        assert_eq!(blocks.len(), 1);
        match &blocks[0].kind {
            BlockKind::FormulaReview { reason, crop_path } => {
                assert!(reason.contains("visual-isolated-equation-band"));
                assert_eq!(
                    crop_path.as_deref(),
                    Some("debug/formulas/page1_formula1.png")
                );
            }
            other => panic!("expected formula review block, got {other:?}"),
        }
    }
}
