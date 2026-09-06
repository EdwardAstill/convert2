//! Table stage of the conversion pipeline: candidate-to-block conversion,
//! geometry-based table detection, and debug/crop output.

use std::path::Path;

use anyhow::Context;

use crate::config::TableMode;
use crate::document::types::{Bbox, Block, BlockKind, DetectedTable, RawPage, TableRender};
use crate::layout::drawing_ops::extract_lines;
use crate::layout::table::TableCandidate;
use crate::layout::table_detector::{detect_table_region_candidates, GeometryTableRegion};

pub(super) fn table_candidates_to_blocks(
    page_num: usize,
    candidates: Vec<TableCandidate>,
) -> Vec<Block> {
    candidates
        .into_iter()
        .enumerate()
        .map(|(idx, candidate)| {
            Block::special(
                2_000_000 + idx,
                candidate.table.bbox,
                BlockKind::CoordinateTable {
                    table: candidate.table,
                },
                page_num,
                0.0,
                "table".to_string(),
            )
        })
        .collect()
}

pub(super) fn is_broad_layout_table_candidate(
    candidate: &TableCandidate,
    page_height: f32,
) -> bool {
    candidate.is_broad_layout_candidate(page_height)
}

pub(super) fn detect_geometry_table_candidates(
    mu_doc: Option<&mupdf::Document>,
    raw_page: &RawPage,
    mode: TableMode,
) -> Vec<TableCandidate> {
    if matches!(mode, TableMode::Off) {
        return Vec::new();
    }

    let Some(mu_doc) = mu_doc else {
        return Vec::new();
    };
    let Ok(mu_page) = mu_doc.load_page(raw_page.page_num as i32) else {
        return Vec::new();
    };
    let Ok((hlines, vlines)) = extract_lines(&mu_page, raw_page.width, raw_page.height) else {
        return Vec::new();
    };
    let regions = detect_table_region_candidates(
        &hlines,
        &vlines,
        &raw_page.words,
        raw_page.width,
        raw_page.height,
    );

    regions
        .into_iter()
        .filter_map(|region| geometry_region_to_table_candidate(region, mode))
        .collect()
}

pub(super) fn geometry_region_to_table_candidate(
    region: GeometryTableRegion,
    mode: TableMode,
) -> Option<TableCandidate> {
    let render = match mode {
        TableMode::Off => return None,
        TableMode::Layout => TableRender::Layout {
            text: region.layout_text,
        },
        TableMode::Native => TableRender::Markdown,
        TableMode::Auto if region.row_consistency >= 0.80 && region.rows.len() >= 3 => {
            TableRender::Markdown
        }
        TableMode::Auto => TableRender::Layout {
            text: region.layout_text,
        },
    };

    Some(TableCandidate {
        table: DetectedTable {
            bbox: region.bbox,
            rows: region.rows,
            confidence: region.confidence,
            render,
        },
        source_block_ids: region.source_block_ids,
        evidence: region.evidence,
    })
}

pub(super) fn write_table_debug(
    output_dir: &Path,
    page_num: usize,
    candidates: &[TableCandidate],
) -> anyhow::Result<()> {
    #[derive(serde::Serialize)]
    struct DebugTable<'a> {
        table_region: Bbox,
        confidence: f32,
        render: &'a TableRender,
        evidence: &'a crate::layout::table::TableEvidence,
        rows: &'a [Vec<String>],
    }

    let debug_dir = output_dir.join("debug").join("tables");
    std::fs::create_dir_all(&debug_dir)
        .with_context(|| format!("Failed to create table debug dir {}", debug_dir.display()))?;
    let path = debug_dir.join(format!("page{}.json", page_num + 1));
    let tables: Vec<_> = candidates
        .iter()
        .map(|candidate| DebugTable {
            table_region: candidate.table.bbox,
            confidence: candidate.table.confidence,
            render: &candidate.table.render,
            evidence: &candidate.evidence,
            rows: &candidate.table.rows,
        })
        .collect();
    let json = serde_json::to_string_pretty(&tables)?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write table debug {}", path.display()))
}

pub(super) fn write_table_crops(
    pdf_path: &Path,
    output_dir: &Path,
    page_num: usize,
    candidates: &[TableCandidate],
    dpi: u32,
) -> anyhow::Result<()> {
    let tables_dir = output_dir.join("tables");
    std::fs::create_dir_all(&tables_dir)
        .with_context(|| format!("Failed to create tables dir {}", tables_dir.display()))?;

    if candidates.is_empty() {
        return Ok(());
    }

    let document = mupdf::Document::open(pdf_path).with_context(|| {
        format!(
            "Failed to open {} for table crop rendering",
            pdf_path.display()
        )
    })?;
    let page = document.load_page(page_num as i32).with_context(|| {
        format!(
            "Failed to load page {} for table crop rendering",
            page_num + 1
        )
    })?;

    for (index, candidate) in candidates.iter().enumerate() {
        let filename = format!("page{}_table{}.png", page_num + 1, index + 1);
        let abs_path = tables_dir.join(&filename);
        if let Some(bytes) =
            crate::figure::render::render_bbox_png(&page, candidate.table.bbox, dpi)
                .with_context(|| format!("Failed to render table crop {filename}"))?
        {
            std::fs::write(&abs_path, bytes)
                .with_context(|| format!("Failed to write table crop {}", abs_path.display()))?;
        }
    }

    Ok(())
}
