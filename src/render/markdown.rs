#![allow(dead_code)]

use crate::document::types::{Block, BlockKind, Document, ExtractedImage, Page, Section};
use crate::error::VtvResult;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The output of rendering a document — markdown text plus structured sections.
pub struct RenderedDocument {
    /// Full document markdown (all pages concatenated).
    pub markdown: String,
    /// Document split into sections (by heading boundaries).
    pub sections: Vec<Section>,
    /// Images extracted to disk during rendering.
    pub images: Vec<ExtractedImage>,
    /// Source document path.
    pub source_path: PathBuf,
}

pub struct MarkdownRenderer {
    /// Whether to extract images to disk. False = skip image extraction.
    pub extract_images: bool,
    /// Directory to write extracted images into (e.g. `output/images/`).
    pub image_output_dir: Option<PathBuf>,
}

struct ScholarlyFrontMatterRender {
    markdown: String,
    next_index: usize,
}

#[derive(Default)]
struct RenderContext {
    repeated_edge_text_first_seen: HashMap<String, usize>,
    repeated_media_fingerprints: HashSet<MediaFingerprint>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct MediaFingerprint {
    center_x_bin: u8,
    center_y_bin: u8,
    width_bin: u8,
    height_bin: u8,
    edge_band: u8,
}

#[derive(Default)]
struct PageMediaPlan {
    kept_block_ids: HashSet<usize>,
}

struct StructuredRender {
    markdown: String,
    next_index: usize,
}

#[derive(Clone, Debug)]
struct ParsedNumericRow {
    label: String,
    values: Vec<String>,
}

impl MarkdownRenderer {
    pub fn new(extract_images: bool, image_output_dir: Option<PathBuf>) -> Self {
        Self {
            extract_images,
            image_output_dir,
        }
    }

    pub fn render_document(&self, doc: &Document) -> VtvResult<RenderedDocument> {
        let mut all_markdown = String::new();
        let mut all_images: Vec<ExtractedImage> = Vec::new();
        let mut image_counter = 0usize;
        let render_ctx = build_render_context(doc);

        for page in &doc.pages {
            let (page_md, mut page_images) =
                self.render_page(page, &render_ctx, &doc.source_path, &mut image_counter)?;
            all_markdown.push_str(&page_md);
            all_images.append(&mut page_images);
        }

        let sections = split_into_sections(&all_markdown, doc);

        Ok(RenderedDocument {
            markdown: all_markdown,
            sections,
            images: all_images,
            source_path: doc.source_path.clone(),
        })
    }

    fn render_page(
        &self,
        page: &Page,
        render_ctx: &RenderContext,
        _source_pdf: &Path,
        _image_counter: &mut usize,
    ) -> VtvResult<(String, Vec<ExtractedImage>)> {
        let mut md = String::new();
        let images: Vec<ExtractedImage> = Vec::new();

        // Emit a page boundary marker (1-indexed, invisible when rendered)
        md.push_str(&format!("<!-- page:{} -->\n", page.page_num + 1));

        // Hybrid override: if this page was routed through the external
        // backend, emit its markdown verbatim and skip the block-based render.
        if let Some(override_md) = &page.override_markdown {
            md.push_str(override_md.trim_end());
            md.push_str("\n\n");
            return Ok((md, images));
        }

        // Warn if page has no extractable text (scanned image or empty page)
        if page.blocks.is_empty() {
            md.push_str(&format!(
                "<!-- WARNING: page {} has no extractable text (scanned image or empty page) -->\n",
                page.page_num + 1
            ));
        }

        // Sort blocks by reading_order
        let mut blocks: Vec<&Block> = page.blocks.iter().collect();
        blocks.sort_by_key(|b| b.reading_order);
        let scholarly_front_page = page.page_num == 0
            && blocks
                .iter()
                .any(|block| is_abstract_heading(block.text.trim()));
        let media_plan = build_page_media_plan(page, &blocks, render_ctx, scholarly_front_page);

        let mut i = 0;
        if let Some(front_matter) = render_scholarly_first_page(&blocks, page, &media_plan) {
            md.push_str(&front_matter.markdown);
            i = front_matter.next_index;
        }

        while i < blocks.len() {
            if let Some(structured) = try_render_structured_region(&blocks, i) {
                md.push_str(&structured.markdown);
                i = structured.next_index;
                continue;
            }

            let block = blocks[i];
            if should_suppress_repeated_text_block(block, page, render_ctx) {
                i += 1;
                continue;
            }
            match &block.kind {
                BlockKind::Heading { level } => {
                    md.push_str(&render_heading(&block.text, *level));
                    i += 1;
                }
                BlockKind::Paragraph => {
                    let text = block.text.trim();
                    if !text.is_empty() {
                        md.push_str(text);
                        md.push_str("\n\n");
                    }
                    i += 1;
                }
                BlockKind::ListItem { .. } => {
                    // Collect consecutive list items
                    let mut list_blocks = vec![block];
                    let mut j = i + 1;
                    while j < blocks.len() {
                        if matches!(&blocks[j].kind, BlockKind::ListItem { .. }) {
                            list_blocks.push(blocks[j]);
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    md.push_str(&render_list(&list_blocks));
                    md.push('\n');
                    i = j;
                }
                BlockKind::TableCell { .. } => {
                    // Collect all consecutive table cells
                    let mut table_blocks = vec![block];
                    let mut j = i + 1;
                    while j < blocks.len() {
                        if matches!(&blocks[j].kind, BlockKind::TableCell { .. }) {
                            table_blocks.push(blocks[j]);
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    md.push_str(&render_table(&table_blocks));
                    md.push('\n');
                    i = j;
                }
                BlockKind::Caption => {
                    md.push('*');
                    md.push_str(block.text.trim());
                    md.push_str("*\n\n");
                    i += 1;
                }
                BlockKind::CodeBlock => {
                    md.push_str("```\n");
                    md.push_str(block.text.trim());
                    md.push_str("\n```\n\n");
                    i += 1;
                }
                BlockKind::Image { path } => {
                    if media_plan.kept_block_ids.contains(&block.id) {
                        if let Some(p) = path {
                            md.push_str(&format!("![image]({})\n\n", p));
                        }
                    }
                    i += 1;
                }
                BlockKind::Figure { path, caption } => {
                    if media_plan.kept_block_ids.contains(&block.id) {
                        if let Some(p) = path {
                            md.push_str(&format!("![image]({})\n\n", p));
                        }
                        if let Some(c) = caption {
                            md.push('*');
                            md.push_str(c.trim());
                            md.push_str("*\n\n");
                        }
                    }
                    i += 1;
                }
                BlockKind::Formula { latex, display } => {
                    if *display {
                        md.push_str(&format!("$$ {} $$\n\n", latex.trim()));
                    } else {
                        md.push_str(&format!("${}$\n\n", latex.trim()));
                    }
                    i += 1;
                }
                // Skip navigation artifacts
                BlockKind::PageNumber | BlockKind::RunningHeader | BlockKind::RunningFooter => {
                    i += 1;
                }
            }
        }

        Ok((md, images))
    }
}

fn build_render_context(doc: &Document) -> RenderContext {
    let mut repeated_text_pages: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut first_seen_text_page: HashMap<String, usize> = HashMap::new();
    let mut repeated_media_pages: HashMap<MediaFingerprint, HashSet<usize>> = HashMap::new();

    for page in &doc.pages {
        for block in &page.blocks {
            if let Some(key) = normalized_repeated_text_key(block, page) {
                repeated_text_pages
                    .entry(key.clone())
                    .or_default()
                    .insert(page.page_num);
                first_seen_text_page
                    .entry(key)
                    .and_modify(|first_seen| *first_seen = (*first_seen).min(page.page_num))
                    .or_insert(page.page_num);
            }

            if is_media_block(block) {
                repeated_media_pages
                    .entry(media_fingerprint(block, page))
                    .or_default()
                    .insert(page.page_num);
            }
        }
    }

    RenderContext {
        repeated_edge_text_first_seen: repeated_text_pages
            .into_iter()
            .filter_map(|(key, pages)| {
                (pages.len() >= 3).then_some((key.clone(), *first_seen_text_page.get(&key)?))
            })
            .collect(),
        repeated_media_fingerprints: repeated_media_pages
            .into_iter()
            .filter_map(|(fingerprint, pages)| (pages.len() >= 2).then_some(fingerprint))
            .collect(),
    }
}

fn build_page_media_plan(
    page: &Page,
    blocks: &[&Block],
    render_ctx: &RenderContext,
    scholarly_front_page: bool,
) -> PageMediaPlan {
    let media_blocks: Vec<&Block> = blocks
        .iter()
        .copied()
        .filter(|block| is_media_block(block))
        .collect();
    if media_blocks.is_empty() {
        return PageMediaPlan::default();
    }

    let content_block_count = blocks
        .iter()
        .copied()
        .filter(|block| {
            !is_media_block(block) && !should_suppress_repeated_text_block(block, page, render_ctx)
        })
        .count();
    let short_heading_count = blocks
        .iter()
        .copied()
        .filter(|block| {
            matches!(block.kind, BlockKind::Heading { .. })
                && block.text.split_whitespace().count() <= 12
        })
        .count();
    let layout_heavy = media_blocks.len() >= 6
        || (media_blocks.len() >= 4 && content_block_count <= 10)
        || short_heading_count >= 4;

    let mut cap = if scholarly_front_page || media_blocks.len() >= 10 {
        1usize
    } else if layout_heavy {
        2usize
    } else if media_blocks.len() >= 4 {
        3usize
    } else {
        media_blocks.len()
    };

    #[derive(Clone, Copy)]
    struct Candidate {
        block_id: usize,
        score: i32,
        hard_drop: bool,
        keep_even_if_capped: bool,
    }

    let mut candidates: Vec<Candidate> = media_blocks
        .iter()
        .map(|block| {
            let has_caption = media_has_caption(block, blocks, page);
            let bbox = block.bbox;
            let page_area = (page.width.max(1.0) * page.height.max(1.0)).max(1.0);
            let area_ratio = (bbox.area() / page_area).clamp(0.0, 1.0);
            let width_ratio = (bbox.width() / page.width.max(1.0)).clamp(0.0, 1.0);
            let height_ratio = (bbox.height() / page.height.max(1.0)).clamp(0.0, 1.0);
            let aspect_ratio = bbox.width().max(1.0) / bbox.height().max(1.0);
            let top_band = bbox.y1 <= page.height * 0.18;
            let bottom_band = bbox.y0 >= page.height * 0.84;
            let edge_band = top_band
                || bottom_band
                || bbox.x0 <= page.width * 0.06
                || bbox.x1 >= page.width * 0.94;
            let repeated = render_ctx
                .repeated_media_fingerprints
                .contains(&media_fingerprint(block, page));
            let tiny = area_ratio < 0.015 || (width_ratio < 0.12 && height_ratio < 0.12);
            let extreme_aspect = aspect_ratio >= 5.5 || aspect_ratio <= 0.22;
            let top_half = bbox.center_y() <= page.height * 0.55;
            let centered_body = bbox.center_y() > page.height * 0.18
                && bbox.center_y() < page.height * 0.82
                && bbox.center_x() > page.width * 0.12
                && bbox.center_x() < page.width * 0.88;

            let mut score = (area_ratio * 100.0).round() as i32;
            if matches!(block.kind, BlockKind::Figure { .. }) {
                score += 15;
            }
            if has_caption {
                score += 55;
            }
            if area_ratio >= 0.12 {
                score += 20;
            }
            if centered_body {
                score += 8;
            }
            if tiny {
                score -= 30;
            }
            if edge_band {
                score -= 20;
            }
            if repeated {
                score -= 30;
            }
            if extreme_aspect && area_ratio < 0.08 {
                score -= 20;
            }
            if layout_heavy && !has_caption {
                score -= 12;
            }
            if scholarly_front_page && top_half && !has_caption {
                score -= 55;
            }

            let hard_drop = (!has_caption && tiny && edge_band)
                || (!has_caption && repeated && (edge_band || scholarly_front_page))
                || (scholarly_front_page && top_half && !has_caption && area_ratio < 0.16)
                || (layout_heavy && !has_caption && repeated && area_ratio < 0.06);

            Candidate {
                block_id: block.id,
                score,
                hard_drop,
                keep_even_if_capped: has_caption && !bottom_band,
            }
        })
        .collect();

    let guaranteed = candidates
        .iter()
        .filter(|candidate| candidate.keep_even_if_capped && !candidate.hard_drop)
        .count();
    cap = cap.max(guaranteed);

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.block_id.cmp(&right.block_id))
    });

    PageMediaPlan {
        kept_block_ids: candidates
            .into_iter()
            .filter(|candidate| !candidate.hard_drop)
            .take(cap)
            .map(|candidate| candidate.block_id)
            .collect(),
    }
}

fn normalized_repeated_text_key(block: &Block, page: &Page) -> Option<String> {
    if !matches!(block.kind, BlockKind::Heading { .. } | BlockKind::Paragraph) {
        return None;
    }
    if block.bbox.y0 > page.height * 0.18 && block.bbox.y1 < page.height * 0.84 {
        return None;
    }

    let normalized = block
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase();
    let alnum_count = normalized.chars().filter(|ch| ch.is_alphanumeric()).count();
    if normalized.is_empty() || normalized.len() > 120 || alnum_count < 4 {
        return None;
    }
    Some(normalized)
}

fn should_suppress_repeated_text_block(
    block: &Block,
    page: &Page,
    render_ctx: &RenderContext,
) -> bool {
    normalized_repeated_text_key(block, page)
        .and_then(|key| {
            render_ctx
                .repeated_edge_text_first_seen
                .get(&key)
                .copied()
                .map(|first_seen| page.page_num > first_seen)
        })
        .unwrap_or(false)
}

fn is_media_block(block: &Block) -> bool {
    matches!(
        block.kind,
        BlockKind::Image { .. } | BlockKind::Figure { .. }
    )
}

fn media_has_caption(block: &Block, blocks: &[&Block], page: &Page) -> bool {
    if matches!(
        block.kind,
        BlockKind::Figure {
            caption: Some(ref caption),
            ..
        } if !caption.trim().is_empty()
    ) {
        return true;
    }

    blocks.iter().copied().any(|other| {
        matches!(other.kind, BlockKind::Caption)
            && other.bbox.x0 < block.bbox.x1
            && other.bbox.x1 > block.bbox.x0
            && {
                let gap = if other.bbox.y0 >= block.bbox.y1 {
                    other.bbox.y0 - block.bbox.y1
                } else if block.bbox.y0 >= other.bbox.y1 {
                    block.bbox.y0 - other.bbox.y1
                } else {
                    0.0
                };
                gap <= page.height * 0.06
            }
    })
}

fn media_fingerprint(block: &Block, page: &Page) -> MediaFingerprint {
    fn quantize(value: f32) -> u8 {
        (value.clamp(0.0, 1.0) * 10.0).round() as u8
    }

    let width = page.width.max(1.0);
    let height = page.height.max(1.0);
    let bbox = block.bbox;
    let edge_band = if bbox.y1 <= page.height * 0.18 {
        0
    } else if bbox.y0 >= page.height * 0.84 {
        1
    } else {
        2
    };

    MediaFingerprint {
        center_x_bin: quantize(bbox.center_x() / width),
        center_y_bin: quantize(bbox.center_y() / height),
        width_bin: quantize(bbox.width() / width),
        height_bin: quantize(bbox.height() / height),
        edge_band,
    }
}

fn render_heading(text: &str, level: u8) -> String {
    let prefix = "#".repeat(level.clamp(1, 6) as usize);
    format!("{} {}\n\n", prefix, normalize_heading_text(text.trim()))
}

fn normalize_heading_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    let chars: Vec<char> = text.chars().collect();

    for (idx, ch) in chars.iter().copied().enumerate() {
        let prev = idx.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(idx + 1).copied();

        let should_insert_space = match (prev, ch, next) {
            (Some(prev), ch, Some(next))
                if prev.is_ascii_digit()
                    && !prev.is_whitespace()
                    && ch.is_uppercase()
                    && next.is_lowercase() =>
            {
                let prev_prev = idx.checked_sub(2).and_then(|i| chars.get(i)).copied();
                !matches!(prev_prev, Some(p) if p.is_alphabetic())
            }
            (Some('.'), ch, Some(next)) if ch.is_uppercase() && next.is_lowercase() => {
                let prev_prev = idx.checked_sub(2).and_then(|i| chars.get(i)).copied();
                matches!(prev_prev, Some(p) if p.is_ascii_digit())
            }
            _ => false,
        };

        if should_insert_space && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(ch);
    }

    out
}

fn render_list(blocks: &[&Block]) -> String {
    let mut result = String::new();
    let mut ordered_counters: std::collections::HashMap<u8, usize> =
        std::collections::HashMap::new();

    for block in blocks {
        if let BlockKind::ListItem { ordered, depth } = &block.kind {
            let indent = "  ".repeat(*depth as usize);
            if *ordered {
                let counter = ordered_counters.entry(*depth).or_insert(0);
                *counter += 1;
                result.push_str(&format!("{}{}. {}\n", indent, counter, block.text.trim()));
            } else {
                result.push_str(&format!("{}- {}\n", indent, block.text.trim()));
            }
        }
    }
    result
}

fn render_table(blocks: &[&Block]) -> String {
    let (grid, max_col) = build_table_grid(blocks);
    if grid.is_empty() {
        return String::new();
    }

    if let Some(rendered) = render_key_value_grid(&grid, max_col + 1) {
        return rendered;
    }

    render_table_grid(&grid, max_col + 1)
}

fn build_table_grid(
    blocks: &[&Block],
) -> (
    std::collections::BTreeMap<usize, std::collections::BTreeMap<usize, String>>,
    usize,
) {
    use std::collections::BTreeMap;

    let mut grid: BTreeMap<usize, BTreeMap<usize, String>> = BTreeMap::new();
    let mut max_col = 0usize;

    for block in blocks {
        if let BlockKind::TableCell { row, col } = block.kind {
            grid.entry(row)
                .or_default()
                .insert(col, block.text.trim().to_string());
            max_col = max_col.max(col);
        }
    }

    (grid, max_col)
}

fn render_table_grid(
    grid: &std::collections::BTreeMap<usize, std::collections::BTreeMap<usize, String>>,
    col_count: usize,
) -> String {
    let mut result = String::new();
    let mut first_row = true;

    for row_cells in grid.values() {
        result.push('|');
        for col in 0..col_count {
            let cell = row_cells.get(&col).map(String::as_str).unwrap_or("");
            result.push_str(&format!(" {} |", cell));
        }
        result.push('\n');

        // Insert separator after header row
        if first_row {
            result.push('|');
            for _ in 0..col_count {
                result.push_str(" --- |");
            }
            result.push('\n');
            first_row = false;
        }
    }

    result
}

fn render_key_value_grid(
    grid: &std::collections::BTreeMap<usize, std::collections::BTreeMap<usize, String>>,
    col_count: usize,
) -> Option<String> {
    if col_count != 2 || grid.len() < 2 {
        return None;
    }

    let mut rendered = String::new();
    let mut pair_count = 0usize;

    for row_cells in grid.values() {
        let key = row_cells.get(&0).map(String::as_str).unwrap_or("").trim();
        let value = row_cells.get(&1).map(String::as_str).unwrap_or("").trim();

        if key.is_empty() || value.is_empty() || !looks_like_key_value_label(key) {
            return None;
        }

        rendered.push_str(&format!(
            "- {}: {}\n",
            normalize_field_label(key),
            normalize_field_value(value)
        ));
        pair_count += 1;
    }

    if pair_count >= 2 {
        rendered.push('\n');
        Some(rendered)
    } else {
        None
    }
}

fn try_render_structured_region(blocks: &[&Block], start: usize) -> Option<StructuredRender> {
    match blocks[start].kind {
        BlockKind::TableCell { .. } => {
            let mut end = start + 1;
            while end < blocks.len() && matches!(blocks[end].kind, BlockKind::TableCell { .. }) {
                end += 1;
            }
            Some(StructuredRender {
                markdown: format!("{}\n", render_table(&blocks[start..end])),
                next_index: end,
            })
        }
        BlockKind::Paragraph | BlockKind::ListItem { .. } => {
            try_render_implicit_numeric_table(blocks, start)
                .or_else(|| try_render_form_fields(blocks, start))
        }
        _ => None,
    }
}

fn try_render_implicit_numeric_table(blocks: &[&Block], start: usize) -> Option<StructuredRender> {
    let end = collect_textish_run(blocks, start, 18);
    let run = &blocks[start..end];
    if run.len() < 3 {
        return None;
    }

    let mut count_histogram = std::collections::BTreeMap::new();
    let mut parsed_rows: Vec<Option<ParsedNumericRow>> = Vec::with_capacity(run.len());

    for block in run {
        let parsed = parse_numeric_row(block.text.trim());
        if let Some(row) = &parsed {
            if row.values.len() >= 2 {
                *count_histogram.entry(row.values.len()).or_insert(0usize) += 1;
            }
        }
        parsed_rows.push(parsed);
    }

    let dominant_count = count_histogram
        .into_iter()
        .max_by_key(|(count, hits)| (*hits, *count))
        .and_then(|(count, hits)| if hits >= 3 { Some(count) } else { None })?;
    let parsed_rows: Vec<Option<ParsedNumericRow>> = parsed_rows
        .into_iter()
        .map(|row| row.map(|row| normalize_numeric_row(row, dominant_count)))
        .collect();

    let mut header_index = None;
    if parsed_rows
        .first()
        .and_then(|row| row.as_ref())
        .map(|row| row.values.is_empty())
        .unwrap_or(false)
        && looks_like_table_header(run[0].text.trim(), dominant_count)
    {
        header_index = Some(0usize);
    }

    let data_start = header_index.map(|idx| idx + 1).unwrap_or(0);
    let mut data_end = data_start;
    while data_end < parsed_rows.len() {
        match &parsed_rows[data_end] {
            Some(row) if row.values.len() == dominant_count => data_end += 1,
            _ => break,
        }
    }

    if data_end - data_start < 3 {
        return None;
    }

    let invoice_header = header_index
        .map(|idx| looks_like_invoice_header(run[idx].text.trim()))
        .unwrap_or(false);
    let headers = derive_table_headers(
        header_index.map(|idx| run[idx].text.trim()),
        dominant_count,
        invoice_header,
    );

    let mut markdown = String::new();
    markdown.push('|');
    for header in &headers {
        markdown.push_str(&format!(" {} |", header));
    }
    markdown.push('\n');
    markdown.push('|');
    for _ in &headers {
        markdown.push_str(" --- |");
    }
    markdown.push('\n');

    for parsed in parsed_rows[data_start..data_end].iter().flatten() {
        markdown.push('|');
        markdown.push_str(&format!(" {} |", escape_table_cell(&parsed.label)));
        for value in &parsed.values {
            markdown.push_str(&format!(" {} |", escape_table_cell(value)));
        }
        markdown.push('\n');
    }

    let mut consumed = data_end;
    if let Some(total) = run
        .get(data_end)
        .and_then(|block| extract_single_numeric_value(block.text.trim()))
        .filter(|_| invoice_header)
    {
        markdown.push('\n');
        markdown.push_str(&format!("Total: {}\n", total));
        consumed += 1;
    }

    markdown.push('\n');

    Some(StructuredRender {
        markdown,
        next_index: start + consumed,
    })
}

fn try_render_form_fields(blocks: &[&Block], start: usize) -> Option<StructuredRender> {
    let end = collect_textish_run(blocks, start, 12);
    let run = &blocks[start..end];
    if run.len() < 3 {
        return None;
    }

    let mut markdown = String::new();
    let mut idx = 0usize;
    let mut label_count = 0usize;

    while idx < run.len() {
        let text = normalize_structured_text(run[idx].text.trim());
        if !looks_like_field_label(&text) {
            break;
        }

        let label = normalize_field_label(&text);
        let mut value = extract_inline_field_value(&text).map(str::to_string);
        let mut consumed = 1usize;

        if value.is_none() {
            if let Some(next_text) = run
                .get(idx + 1)
                .map(|block| normalize_structured_text(block.text.trim()))
            {
                if looks_like_choice_line(&next_text) {
                    value = Some(next_text.split_whitespace().collect::<Vec<_>>().join(" | "));
                    consumed = 2;
                } else if looks_like_field_value(&next_text) {
                    value = Some(next_text);
                    consumed = 2;
                }
            }
        }

        markdown.push_str(&format!(
            "- {}: {}\n",
            label,
            value.unwrap_or_else(|| "________".to_string())
        ));
        label_count += 1;
        idx += consumed;
    }

    if label_count >= 3 {
        markdown.push('\n');
        Some(StructuredRender {
            markdown,
            next_index: start + idx,
        })
    } else {
        None
    }
}

fn collect_textish_run(blocks: &[&Block], start: usize, limit: usize) -> usize {
    let mut end = start;
    while end < blocks.len()
        && end - start < limit
        && matches!(
            blocks[end].kind,
            BlockKind::Paragraph | BlockKind::ListItem { .. }
        )
    {
        end += 1;
    }
    end
}

fn parse_numeric_row(text: &str) -> Option<ParsedNumericRow> {
    let trimmed = strip_leading_list_marker(&normalize_structured_text(text));
    if trimmed.is_empty() {
        return None;
    }

    let matches: Vec<_> = numeric_value_re().find_iter(&trimmed).collect();
    if matches.is_empty() {
        return Some(ParsedNumericRow {
            label: trimmed,
            values: Vec::new(),
        });
    }

    let mut trailing: Vec<(usize, usize, String)> = Vec::new();
    let mut next_end = trimmed.len();
    for found in matches.into_iter().rev() {
        if !is_numeric_separator(&trimmed[found.end()..next_end]) {
            if trailing.is_empty() {
                continue;
            }
            break;
        }
        trailing.push((found.start(), found.end(), found.as_str().to_string()));
        next_end = found.start();
    }
    trailing.reverse();

    let Some((first_start, _, _)) = trailing.first() else {
        return None;
    };
    let label = trimmed[..*first_start]
        .trim()
        .trim_end_matches(':')
        .trim()
        .to_string();
    if label.is_empty() {
        return None;
    }

    Some(ParsedNumericRow {
        label,
        values: trailing.into_iter().map(|(_, _, value)| value).collect(),
    })
}

fn normalize_numeric_row(mut row: ParsedNumericRow, dominant_count: usize) -> ParsedNumericRow {
    if row.values.len() == 1 {
        let value = row.values[0].trim();
        if value.len() == dominant_count && value.chars().all(|ch| ch == '0') {
            row.values = vec!["0".to_string(); dominant_count];
        }
    }
    row
}

fn looks_like_table_header(text: &str, value_count: usize) -> bool {
    let upper = text.chars().filter(|ch| ch.is_ascii_uppercase()).count();
    let alpha = text.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
    alpha > 8
        && upper * 2 >= alpha
        && (looks_like_invoice_header(text)
            || (value_count >= 2 && text.split_whitespace().count() >= value_count + 1))
}

fn derive_table_headers(
    header_text: Option<&str>,
    value_count: usize,
    invoice_header: bool,
) -> Vec<String> {
    if invoice_header {
        let mut headers = vec!["Item".to_string()];
        headers.extend(
            ["Quantity", "Price", "Amount"]
                .iter()
                .take(value_count)
                .map(|s| s.to_string()),
        );
        if value_count > 3 {
            for idx in 4..=value_count {
                headers.push(format!("Value {idx}"));
            }
        }
        return headers;
    }

    if let Some(text) = header_text {
        if text.to_ascii_lowercase().contains("descr") && value_count == 4 {
            return vec![
                "Item".to_string(),
                "Value 1".to_string(),
                "Value 2".to_string(),
                "Value 3".to_string(),
                "Value 4".to_string(),
            ];
        }
    }

    let mut headers = vec!["Item".to_string()];
    for idx in 1..=value_count {
        headers.push(format!("Value {idx}"));
    }
    headers
}

fn escape_table_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn numeric_value_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\(?-?(?:\d{1,3}(?:[.,]\d{3})+(?:[.,]\d{2})?|\d+(?:[.,]\d{2})|\d+)\)?").unwrap()
    })
}

fn invoice_header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(item|description|quantity|price|amount|invoice)\b").unwrap()
    })
}

fn looks_like_invoice_header(text: &str) -> bool {
    invoice_header_re().find_iter(text).count() >= 2
}

fn extract_single_numeric_value(text: &str) -> Option<String> {
    let trimmed = normalize_structured_text(text);
    let matches: Vec<_> = numeric_value_re().find_iter(&trimmed).collect();
    if matches.len() == 1 && matches[0].start() == 0 && matches[0].end() == trimmed.len() {
        Some(trimmed)
    } else {
        None
    }
}

fn is_numeric_separator(text: &str) -> bool {
    text.chars()
        .all(|ch| ch.is_whitespace() || matches!(ch, '|' | '/' | '\\'))
}

fn normalize_structured_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_leading_list_marker(text: &str) -> String {
    let trimmed = text.trim_start();
    let mut digit_end = 0usize;
    for (idx, ch) in trimmed.char_indices() {
        if ch.is_ascii_digit() {
            digit_end = idx + ch.len_utf8();
        } else {
            break;
        }
    }

    if digit_end > 0 {
        let remainder = &trimmed[digit_end..];
        if let Some(marker) = remainder.chars().next() {
            if matches!(marker, '.' | ')') {
                let after_marker = &remainder[marker.len_utf8()..];
                if after_marker.starts_with(char::is_whitespace) {
                    let rest = after_marker.trim_start();
                    if !rest.is_empty() {
                        return rest.to_string();
                    }
                }
            }
        }
    }

    trimmed.to_string()
}

fn looks_like_field_label(text: &str) -> bool {
    let trimmed = text.trim();
    if !trimmed.ends_with(':') {
        return false;
    }

    trimmed.len() <= 48 && trimmed.split_whitespace().count() <= 6 && !trimmed.contains('.')
}

fn looks_like_key_value_label(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 48
        && trimmed.split_whitespace().count() <= 6
        && trimmed.chars().any(|ch| ch.is_alphabetic())
        && (trimmed.ends_with(':')
            || trimmed.split_whitespace().count() >= 2
            || key_value_label_keyword_re().is_match(trimmed))
}

fn normalize_field_label(text: &str) -> String {
    strip_leading_list_marker(text)
        .trim()
        .trim_end_matches(':')
        .trim()
        .to_string()
}

fn key_value_label_keyword_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(invoice|date|number|phone|email|address|total|due|customer|account)\b")
            .unwrap()
    })
}

fn extract_inline_field_value(text: &str) -> Option<&str> {
    let (label, value) = text.split_once(':')?;
    if !looks_like_field_label(&format!("{}:", label.trim())) {
        return None;
    }
    let value = value.trim();
    if value.is_empty() || value.len() > 60 || value.contains('.') {
        None
    } else {
        Some(value)
    }
}

fn looks_like_choice_line(text: &str) -> bool {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    (2..=8).contains(&tokens.len())
        && tokens.iter().all(|token| {
            !token.contains(':')
                && token.len() <= 18
                && token.chars().any(|ch| ch.is_alphanumeric())
                && !token.chars().any(|ch| matches!(ch, '.' | ','))
        })
}

fn looks_like_field_value(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && !looks_like_field_label(trimmed)
        && !looks_like_choice_line(trimmed)
        && trimmed.len() <= 48
        && trimmed.split_whitespace().count() <= 6
}

fn normalize_field_value(text: &str) -> String {
    strip_leading_list_marker(&normalize_structured_text(text))
}

fn scholarly_metadata_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^\s*(arxiv:\S+|doi:\S+|https?://doi\.org/\S+|preprint\b|published as\b|accepted at\b)",
        )
        .unwrap()
    })
}

fn scholarly_note_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(permission to make digital|all rights reserved|copyright held by|acm isbn|provided proper attribution|equal contribution|author contributions listed|correspondence to:|conference on|neurips|nips|icml|iclr|cvpr|eccv|arxiv,\s*\d{4})",
        )
        .unwrap()
    })
}

fn abstract_heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^\s*abstract(?:\s*[:—–-]\s*.*)?\s*$").unwrap())
}

fn numbered_section_heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^\s*\d+(?:\.\d+)?\.?\s*(?:\d+\.?\s*)?(Introduction|Contents|Summary|Background|Methods?|Approach|Related Work|Experiments?|Results?|Conclusion|Overview)\b",
        )
        .unwrap()
    })
}

fn affiliation_keyword_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(university|institute|department|school|college|research|laborator(?:y|ies)|openai|google|microsoft|meta|deepmind|anthropic|stanford|berkeley|toronto|eth zurich|san francisco)\b",
        )
        .unwrap()
    })
}

fn render_scholarly_first_page(
    blocks: &[&Block],
    page: &Page,
    media_plan: &PageMediaPlan,
) -> Option<ScholarlyFrontMatterRender> {
    if page.page_num != 0 || blocks.len() < 3 {
        return None;
    }

    let title_idx = find_scholarly_title_candidate(blocks, page)?;
    let abstract_idx = blocks
        .iter()
        .position(|block| is_abstract_heading(block.text.trim()));
    let metadata_count = blocks
        .iter()
        .take(16)
        .filter(|block| is_scholarly_metadata_line(block.text.trim()))
        .count();

    if abstract_idx.is_none() && metadata_count == 0 {
        return None;
    }

    let end_idx = find_front_matter_end(blocks, abstract_idx).unwrap_or(blocks.len());
    if end_idx <= title_idx {
        return None;
    }

    let mut markdown = String::new();
    markdown.push_str(&render_heading(blocks[title_idx].text.trim(), 1));

    let abstract_boundary = abstract_idx.unwrap_or(end_idx);
    let mut metadata_blocks: Vec<&Block> = Vec::new();
    let mut author_blocks: Vec<&Block> = Vec::new();
    let mut deferred_notes: Vec<&Block> = Vec::new();
    let mut deferred_media: Vec<&Block> = Vec::new();

    for (idx, block) in blocks.iter().enumerate().take(abstract_boundary) {
        if idx == title_idx {
            continue;
        }

        if matches!(
            block.kind,
            BlockKind::Image { .. } | BlockKind::Figure { .. }
        ) {
            deferred_media.push(block);
            continue;
        }

        let text = block.text.trim();
        if text.is_empty() {
            continue;
        }

        match block.kind {
            BlockKind::PageNumber | BlockKind::RunningHeader | BlockKind::RunningFooter => {}
            _ if is_scholarly_metadata_line(text) => metadata_blocks.push(block),
            _ if is_scholarly_note_line(text) => deferred_notes.push(block),
            _ if looks_like_author_block(block)
                || matches!(block.kind, BlockKind::TableCell { .. }) =>
            {
                author_blocks.push(block)
            }
            _ => author_blocks.push(block),
        }
    }

    for block in metadata_blocks {
        append_paragraph(
            &mut markdown,
            &normalize_front_matter_text(block.text.trim()),
        );
    }

    for entry in collect_author_entries(&author_blocks) {
        append_paragraph(&mut markdown, &entry);
    }

    if let Some(abstract_idx) = abstract_idx {
        markdown.push_str("## Abstract\n\n");

        if let Some(inline_body) = split_abstract_block(blocks[abstract_idx].text.trim()) {
            append_paragraph(&mut markdown, inline_body);
        }

        for block in blocks.iter().take(end_idx).skip(abstract_idx + 1) {
            if matches!(
                block.kind,
                BlockKind::Image { .. } | BlockKind::Figure { .. }
            ) {
                deferred_media.push(block);
                continue;
            }

            let text = block.text.trim();
            if text.is_empty() {
                continue;
            }
            match block.kind {
                BlockKind::PageNumber | BlockKind::RunningHeader | BlockKind::RunningFooter => {}
                _ if is_scholarly_note_line(text) => deferred_notes.push(block),
                _ => append_rendered_block(&mut markdown, block, true),
            }
        }
    }

    for block in deferred_notes {
        append_rendered_block(&mut markdown, block, true);
    }

    for block in deferred_media {
        if media_plan.kept_block_ids.contains(&block.id) {
            append_rendered_block(&mut markdown, block, false);
        }
    }

    Some(ScholarlyFrontMatterRender {
        markdown,
        next_index: end_idx,
    })
}

fn find_scholarly_title_candidate(blocks: &[&Block], page: &Page) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    let y_cutoff = page.height * 0.38;

    for (idx, block) in blocks.iter().enumerate() {
        let text = block.text.trim();
        if text.is_empty() || block.bbox.y0 > y_cutoff {
            continue;
        }
        if is_scholarly_metadata_line(text)
            || is_scholarly_note_line(text)
            || is_abstract_heading(text)
            || is_main_section_heading(text)
            || looks_like_affiliation_line(text)
            || matches!(
                block.kind,
                BlockKind::TableCell { .. }
                    | BlockKind::Image { .. }
                    | BlockKind::Figure { .. }
                    | BlockKind::Formula { .. }
                    | BlockKind::PageNumber
                    | BlockKind::RunningHeader
                    | BlockKind::RunningFooter
            )
        {
            continue;
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() < 3 && text.len() < 24 {
            continue;
        }

        let mut score = block.font_size * 6.0 - block.bbox.y0 * 0.03;
        if matches!(block.kind, BlockKind::Heading { .. }) {
            score += 20.0;
        }
        if has_title_stopword(text) {
            score += 6.0;
        }
        if text.len() > 140 {
            score -= 8.0;
        }
        if text.chars().filter(|ch| ch.is_ascii_digit()).count() > 8 {
            score -= 12.0;
        }

        match best {
            Some((_, best_score)) if score <= best_score => {}
            _ => best = Some((idx, score)),
        }
    }

    best.map(|(idx, _)| idx)
}

fn find_front_matter_end(blocks: &[&Block], abstract_idx: Option<usize>) -> Option<usize> {
    let start = abstract_idx.map(|idx| idx + 1).unwrap_or(0);
    (start..blocks.len()).find(|&idx| is_main_section_heading(blocks[idx].text.trim()))
}

fn is_scholarly_metadata_line(text: &str) -> bool {
    scholarly_metadata_re().is_match(text)
}

fn is_scholarly_note_line(text: &str) -> bool {
    scholarly_note_re().is_match(text)
        || text.starts_with("^{")
        || text.starts_with('©')
        || text.starts_with("ACM Reference Format:")
}

fn is_abstract_heading(text: &str) -> bool {
    abstract_heading_re().is_match(text) || split_abstract_block(text).is_some()
}

fn split_abstract_block(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("abstract") {
        return None;
    }

    let abstract_prefix = trimmed.get(..8).unwrap_or(trimmed);
    if !abstract_prefix.eq_ignore_ascii_case("abstract") {
        return None;
    }

    let rest = trimmed[8..]
        .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, ':' | '—' | '–' | '-'))
        .trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

fn is_main_section_heading(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    numbered_section_heading_re().is_match(trimmed)
        || lower == "introduction"
        || lower == "contents"
        || lower == "summary"
        || lower.starts_with("1introduction")
        || lower.starts_with("1. introduction")
        || lower.starts_with("1. 1. introduction")
        || lower.starts_with("1 contents")
}

fn looks_like_author_block(block: &Block) -> bool {
    let text = block.text.trim();
    if text.is_empty() {
        return false;
    }

    matches!(block.kind, BlockKind::TableCell { .. })
        || looks_like_author_name_line(text)
        || looks_like_affiliation_line(text)
}

fn looks_like_author_name_line(text: &str) -> bool {
    let words: Vec<&str> = text
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';'))
        .filter(|word| !word.is_empty())
        .collect();
    if !(2..=16).contains(&words.len()) {
        return false;
    }

    let stopwords = [
        "a", "an", "and", "are", "as", "at", "by", "for", "from", "in", "is", "of", "on", "or",
        "the", "to", "with",
    ];
    let capitalized = words
        .iter()
        .filter(|word| {
            word.chars()
                .find(|ch| ch.is_alphabetic())
                .map(|ch| ch.is_uppercase())
                .unwrap_or(false)
        })
        .count();
    let stopword_count = words
        .iter()
        .filter(|word| stopwords.contains(&word.to_ascii_lowercase().as_str()))
        .count();
    let lowercase_content_words = words
        .iter()
        .filter(|word| {
            let lower = word.to_ascii_lowercase();
            !stopwords.contains(&lower.as_str())
                && word
                    .chars()
                    .find(|ch| ch.is_alphabetic())
                    .map(|ch| ch.is_lowercase())
                    .unwrap_or(false)
        })
        .count();

    capitalized * 5 >= words.len() * 4 && stopword_count == 0 && lowercase_content_words == 0
}

fn looks_like_affiliation_line(text: &str) -> bool {
    text.contains('@') || affiliation_keyword_re().is_match(text)
}

fn has_title_stopword(text: &str) -> bool {
    let lower = format!(" {} ", text.to_ascii_lowercase());
    [
        " a ",
        " about ",
        " against ",
        " all ",
        " and ",
        " are ",
        " for ",
        " from ",
        " in ",
        " is ",
        " of ",
        " on ",
        " the ",
        " to ",
        " with ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn collect_author_entries(blocks: &[&Block]) -> Vec<String> {
    let mut entries = Vec::new();
    let mut idx = 0;

    while idx < blocks.len() {
        if matches!(blocks[idx].kind, BlockKind::TableCell { .. }) {
            let mut table_blocks = Vec::new();
            while idx < blocks.len() && matches!(blocks[idx].kind, BlockKind::TableCell { .. }) {
                table_blocks.push(blocks[idx]);
                idx += 1;
            }
            entries.extend(table_cell_author_entries(&table_blocks));
            continue;
        }

        let text = normalize_front_matter_text(blocks[idx].text.trim());
        if !text.is_empty() {
            entries.push(text);
        }
        idx += 1;
    }

    entries.dedup();
    entries
}

fn table_cell_author_entries(blocks: &[&Block]) -> Vec<String> {
    use std::collections::BTreeMap;

    let mut cells = BTreeMap::new();
    for block in blocks {
        if let BlockKind::TableCell { row, col } = block.kind {
            let text = normalize_front_matter_text(block.text.trim());
            if !text.is_empty() {
                cells.insert((row, col), text);
            }
        }
    }

    cells.into_values().collect()
}

fn normalize_front_matter_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_paragraph(markdown: &mut String, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    markdown.push_str(text.trim());
    markdown.push_str("\n\n");
}

fn append_rendered_block(markdown: &mut String, block: &Block, force_plain_text: bool) {
    match &block.kind {
        BlockKind::Heading { level } if !force_plain_text => {
            markdown.push_str(&render_heading(&block.text, *level));
        }
        BlockKind::Caption => {
            markdown.push('*');
            markdown.push_str(block.text.trim());
            markdown.push_str("*\n\n");
        }
        BlockKind::CodeBlock => {
            markdown.push_str("```\n");
            markdown.push_str(block.text.trim());
            markdown.push_str("\n```\n\n");
        }
        BlockKind::Image { path } => {
            if let Some(path) = path {
                markdown.push_str(&format!("![image]({})\n\n", path));
            }
        }
        BlockKind::Figure { path, caption } => {
            if let Some(path) = path {
                markdown.push_str(&format!("![image]({})\n\n", path));
            }
            if let Some(caption) = caption {
                markdown.push('*');
                markdown.push_str(caption.trim());
                markdown.push_str("*\n\n");
            }
        }
        BlockKind::Formula { latex, display } => {
            if *display {
                markdown.push_str(&format!("$$ {} $$\n\n", latex.trim()));
            } else {
                markdown.push_str(&format!("${}$\n\n", latex.trim()));
            }
        }
        BlockKind::ListItem { ordered, depth } => {
            let indent = "  ".repeat(*depth as usize);
            if *ordered {
                markdown.push_str(&format!("{}1. {}\n\n", indent, block.text.trim()));
            } else {
                markdown.push_str(&format!("{}- {}\n\n", indent, block.text.trim()));
            }
        }
        BlockKind::PageNumber | BlockKind::RunningHeader | BlockKind::RunningFooter => {}
        _ => append_paragraph(markdown, &normalize_front_matter_text(block.text.trim())),
    }
}

/// Split markdown into sections using heading boundaries.
/// Falls back to a single section if no headings are found.
pub fn split_into_sections(markdown: &str, doc: &Document) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current_title = String::from("Document");
    let mut current_level = 1u8;
    let mut current_content = String::new();
    let mut current_page_start = 1usize;
    let mut current_page = 1usize;
    let mut found_heading = false;

    for line in markdown.lines() {
        // Track page transitions via embedded markers
        if let Some(page_num) = parse_page_marker(line) {
            current_page = page_num;
            // Don't add the marker to content
            continue;
        }

        if let Some((level, title)) = parse_heading_line(line) {
            // Flush previous section
            if found_heading || !current_content.trim().is_empty() {
                sections.push(Section {
                    title: current_title.clone(),
                    level: current_level,
                    content: current_content.trim().to_string(),
                    page_start: current_page_start,
                    page_end: current_page,
                });
            }
            current_title = title;
            current_level = level;
            current_content = String::new();
            current_page_start = current_page;
            found_heading = true;
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Flush last section
    if found_heading || !current_content.trim().is_empty() {
        sections.push(Section {
            title: current_title,
            level: current_level,
            content: current_content.trim().to_string(),
            page_start: current_page_start,
            page_end: doc.pages.len(),
        });
    }

    if sections.is_empty() {
        sections.push(Section {
            title: String::from("Document"),
            level: 1,
            content: markdown.trim().to_string(),
            page_start: 1,
            page_end: doc.pages.len(),
        });
    }

    sections
}

/// Detect an embedded page boundary marker of the form `<!-- page:N -->`.
/// Returns the 1-indexed page number, or `None` if the line is not a marker.
fn parse_page_marker(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    if trimmed.starts_with("<!-- page:") && trimmed.ends_with(" -->") {
        let inner = &trimmed["<!-- page:".len()..trimmed.len() - " -->".len()];
        inner.trim().parse::<usize>().ok()
    } else {
        None
    }
}

/// Parse a markdown heading line. Returns `(level, title)` or `None`.
fn parse_heading_line(line: &str) -> Option<(u8, String)> {
    if !line.starts_with('#') {
        return None;
    }
    let trimmed = line.trim_start_matches('#');
    let level = (line.len() - trimmed.len()) as u8;
    if (1..=6).contains(&level) {
        let title = trimmed.trim().to_string();
        if !title.is_empty() {
            return Some((level, title));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::types::{Bbox, Block, BlockKind, Document, DocumentMetadata, Page};
    use std::path::PathBuf;

    fn make_block(id: usize, text: &str, kind: BlockKind, reading_order: usize) -> Block {
        make_block_at(
            id,
            text,
            kind,
            Bbox::new(0.0, 0.0, 100.0, 20.0),
            12.0,
            reading_order,
        )
    }

    fn make_block_at(
        id: usize,
        text: &str,
        kind: BlockKind,
        bbox: Bbox,
        font_size: f32,
        reading_order: usize,
    ) -> Block {
        Block {
            id,
            bbox,
            text: text.to_string(),
            kind,
            font_size,
            font_name: "Helvetica".to_string(),
            page_num: 0,
            reading_order,
        }
    }

    fn make_doc(pages: Vec<Page>) -> Document {
        Document {
            source_path: PathBuf::from("test.pdf"),
            pages,
            metadata: DocumentMetadata::default(),
        }
    }

    #[test]
    fn renders_heading_paragraph() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block(0, "Introduction", BlockKind::Heading { level: 1 }, 0),
                make_block(1, "Hello world.", BlockKind::Paragraph, 1),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();
        assert!(result.markdown.contains("# Introduction"));
        assert!(result.markdown.contains("Hello world."));
    }

    #[test]
    fn skips_page_numbers_and_running_headers() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block(0, "1", BlockKind::PageNumber, 0),
                make_block(1, "Chapter 1", BlockKind::RunningHeader, 1),
                make_block(2, "Body text.", BlockKind::Paragraph, 2),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();
        assert!(!result.markdown.contains("Chapter 1"));
        assert!(result.markdown.contains("Body text."));
    }

    #[test]
    fn splits_sections_by_headings() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block(0, "Intro", BlockKind::Heading { level: 1 }, 0),
                make_block(1, "Some intro text.", BlockKind::Paragraph, 1),
                make_block(2, "Methods", BlockKind::Heading { level: 2 }, 2),
                make_block(3, "We did things.", BlockKind::Paragraph, 3),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();
        assert_eq!(result.sections.len(), 2);
        assert_eq!(result.sections[0].title, "Intro");
        assert_eq!(result.sections[0].level, 1);
        assert_eq!(result.sections[1].title, "Methods");
        assert_eq!(result.sections[1].level, 2);
    }

    #[test]
    fn renders_unordered_list() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block(
                    0,
                    "Apple",
                    BlockKind::ListItem {
                        ordered: false,
                        depth: 0,
                    },
                    0,
                ),
                make_block(
                    1,
                    "Banana",
                    BlockKind::ListItem {
                        ordered: false,
                        depth: 0,
                    },
                    1,
                ),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();
        assert!(result.markdown.contains("- Apple"));
        assert!(result.markdown.contains("- Banana"));
    }

    #[test]
    fn renders_table() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block(0, "Name", BlockKind::TableCell { row: 0, col: 0 }, 0),
                make_block(1, "Age", BlockKind::TableCell { row: 0, col: 1 }, 1),
                make_block(2, "Alice", BlockKind::TableCell { row: 1, col: 0 }, 2),
                make_block(3, "30", BlockKind::TableCell { row: 1, col: 1 }, 3),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();
        assert!(result.markdown.contains("| Name | Age |"));
        assert!(result.markdown.contains("| --- |"));
        assert!(result.markdown.contains("| Alice | 30 |"));
    }

    #[test]
    fn renders_key_value_table_as_field_list() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block(
                    0,
                    "Invoice Number",
                    BlockKind::TableCell { row: 0, col: 0 },
                    0,
                ),
                make_block(1, "2020-10", BlockKind::TableCell { row: 0, col: 1 }, 1),
                make_block(
                    2,
                    "Invoice Date",
                    BlockKind::TableCell { row: 1, col: 0 },
                    2,
                ),
                make_block(
                    3,
                    "January 8, 2020",
                    BlockKind::TableCell { row: 1, col: 1 },
                    3,
                ),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();

        assert!(result.markdown.contains("- Invoice Number: 2020-10"));
        assert!(result.markdown.contains("- Invoice Date: January 8, 2020"));
        assert!(!result.markdown.contains("| --- |"));
    }

    #[test]
    fn renders_implicit_invoice_rows_as_table() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block(
                    0,
                    "ITEM DESCRIPTION QUANTITY PRICE (€) AMOUNT (€)",
                    BlockKind::Paragraph,
                    0,
                ),
                make_block(1, "1 Super Kite 2 20.00 40.00", BlockKind::Paragraph, 1),
                make_block(2, "2 Turbo Flyer 5 40.00 200.00", BlockKind::Paragraph, 2),
                make_block(3, "3 Giga Trash 1 180.00 180.00", BlockKind::Paragraph, 3),
                make_block(4, "420.00", BlockKind::Paragraph, 4),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();

        assert!(result
            .markdown
            .contains("| Item | Quantity | Price | Amount |"));
        assert!(result
            .markdown
            .contains("| 1 Super Kite | 2 | 20.00 | 40.00 |"));
        assert!(result.markdown.contains("Total: 420.00"));
    }

    #[test]
    fn renders_form_labels_as_labeled_entries() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block(0, "Text, required:", BlockKind::Paragraph, 0),
                make_block(1, "Text, digits:", BlockKind::Paragraph, 1),
                make_block(2, "Check boxes:", BlockKind::Paragraph, 2),
                make_block(3, "A   B   C   D", BlockKind::Paragraph, 3),
                make_block(4, "Radio buttons:", BlockKind::Paragraph, 4),
                make_block(5, "yes   no", BlockKind::Paragraph, 5),
                make_block(6, "Drop-down:", BlockKind::Paragraph, 6),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();

        assert!(result.markdown.contains("- Text, required: ________"));
        assert!(result.markdown.contains("- Check boxes: A | B | C | D"));
        assert!(result.markdown.contains("- Radio buttons: yes | no"));
        assert!(result.markdown.contains("- Drop-down: ________"));
    }

    #[test]
    fn normalizes_zero_shorthand_rows_to_dominant_width() {
        let row = ParsedNumericRow {
            label: "11) Variazioni rimanenze materie prime".to_string(),
            values: vec!["0000".to_string()],
        };
        let normalized = normalize_numeric_row(row, 4);
        assert_eq!(normalized.values, vec!["0", "0", "0", "0"]);
    }

    #[test]
    fn parse_heading_line_valid() {
        assert_eq!(
            parse_heading_line("## Hello"),
            Some((2, "Hello".to_string()))
        );
        assert_eq!(parse_heading_line("# Top"), Some((1, "Top".to_string())));
        assert_eq!(parse_heading_line("Not a heading"), None);
        assert_eq!(parse_heading_line("##"), None); // no title after hashes
    }

    #[test]
    fn normalize_heading_text_inserts_missing_space_after_section_number() {
        assert_eq!(normalize_heading_text("1Introduction"), "1 Introduction");
        assert_eq!(normalize_heading_text("2Background"), "2 Background");
        assert_eq!(normalize_heading_text("3.2Subheading"), "3.2 Subheading");
        assert_eq!(
            normalize_heading_text("Appendix A"),
            "Appendix A",
            "non-numbered headings should be unchanged"
        );
    }

    #[test]
    fn strip_leading_list_marker_removes_simple_number_prefixes() {
        assert_eq!(
            strip_leading_list_marker("1. Revenue 120 220"),
            "Revenue 120 220"
        );
        assert_eq!(
            strip_leading_list_marker("12) Accantonamenti per rischi000240.000"),
            "Accantonamenti per rischi000240.000"
        );
        assert_eq!(
            strip_leading_list_marker("17-bis) Utili e perdite"),
            "17-bis) Utili e perdite",
            "non-simple labels should be preserved"
        );
    }

    #[test]
    fn scanned_page_warning_emitted() {
        // A page with no blocks should produce a WARNING comment
        let page = Page {
            page_num: 2, // 0-indexed → page 3 in output
            width: 595.0,
            height: 842.0,
            blocks: vec![],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();
        assert!(
            result.markdown.contains(
                "<!-- WARNING: page 3 has no extractable text (scanned image or empty page) -->"
            ),
            "Expected scanned page warning in markdown, got: {}",
            result.markdown
        );
        // Should also have the page marker
        assert!(result.markdown.contains("<!-- page:3 -->"));
    }

    #[test]
    fn no_warning_for_page_with_blocks() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![make_block(0, "Some text", BlockKind::Paragraph, 0)],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();
        assert!(
            !result.markdown.contains("WARNING"),
            "Should not have WARNING for page with blocks"
        );
    }

    #[test]
    fn scholarly_front_matter_demotes_metadata_and_author_tables() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block_at(
                    0,
                    "arXiv:1706.03762v7  [cs.CL]  2 Aug 2023",
                    BlockKind::Heading { level: 1 },
                    Bbox::new(50.0, 24.0, 545.0, 42.0),
                    16.0,
                    0,
                ),
                make_block_at(
                    1,
                    "Attention Is All You Need",
                    BlockKind::Heading { level: 2 },
                    Bbox::new(70.0, 60.0, 525.0, 90.0),
                    24.0,
                    1,
                ),
                make_block_at(
                    2,
                    "Ashish Vaswani\nGoogle Brain\navaswani@google.com",
                    BlockKind::TableCell { row: 0, col: 0 },
                    Bbox::new(60.0, 118.0, 220.0, 170.0),
                    11.0,
                    2,
                ),
                make_block_at(
                    3,
                    "Noam Shazeer\nGoogle Brain\nnoam@google.com",
                    BlockKind::TableCell { row: 0, col: 1 },
                    Bbox::new(250.0, 118.0, 410.0, 170.0),
                    11.0,
                    3,
                ),
                make_block_at(
                    4,
                    "Abstract",
                    BlockKind::Heading { level: 4 },
                    Bbox::new(80.0, 200.0, 220.0, 220.0),
                    14.0,
                    4,
                ),
                make_block_at(
                    5,
                    "The Transformer replaces recurrence with attention.",
                    BlockKind::Paragraph,
                    Bbox::new(60.0, 228.0, 520.0, 290.0),
                    11.0,
                    5,
                ),
                make_block_at(
                    6,
                    "1. Introduction",
                    BlockKind::Paragraph,
                    Bbox::new(60.0, 330.0, 250.0, 350.0),
                    13.0,
                    6,
                ),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();

        assert!(result.markdown.contains("# Attention Is All You Need"));
        assert!(result
            .markdown
            .contains("arXiv:1706.03762v7  [cs.CL]  2 Aug 2023"));
        assert!(!result.markdown.contains("# arXiv:1706.03762v7"));
        assert!(!result.markdown.contains("| --- |"));
        assert!(result.markdown.contains("Ashish Vaswani\nGoogle Brain"));
        assert!(result.markdown.contains("## Abstract"));
    }

    #[test]
    fn scholarly_front_matter_splits_inline_abstract_heading() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block_at(
                    0,
                    "Practical Attacks against Black-box Code Completion Engines",
                    BlockKind::Heading { level: 3 },
                    Bbox::new(40.0, 40.0, 555.0, 76.0),
                    23.0,
                    0,
                ),
                make_block_at(
                    1,
                    "Slobodan Jenko",
                    BlockKind::Paragraph,
                    Bbox::new(80.0, 96.0, 260.0, 116.0),
                    12.0,
                    1,
                ),
                make_block_at(
                    2,
                    "arXiv:2408.02509v1  [cs.CR]  5 Aug 2024",
                    BlockKind::Paragraph,
                    Bbox::new(100.0, 132.0, 470.0, 150.0),
                    11.0,
                    2,
                ),
                make_block_at(
                    3,
                    "Abstract—Modern code completion engines can be attacked.",
                    BlockKind::Paragraph,
                    Bbox::new(60.0, 192.0, 530.0, 236.0),
                    11.0,
                    3,
                ),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();

        assert!(result.markdown.contains("## Abstract"));
        assert!(result
            .markdown
            .contains("Modern code completion engines can be attacked."));
        assert!(!result.markdown.contains("Abstract—Modern"));
    }

    #[test]
    fn suppresses_repeated_edge_text_after_first_occurrence() {
        let make_page = |page_num: usize, body: &str| Page {
            page_num,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block_at(
                    page_num * 10,
                    "www.pdfa.org",
                    BlockKind::Paragraph,
                    Bbox::new(40.0, 10.0, 180.0, 28.0),
                    10.0,
                    0,
                ),
                make_block_at(
                    page_num * 10 + 1,
                    body,
                    BlockKind::Paragraph,
                    Bbox::new(60.0, 220.0, 520.0, 280.0),
                    12.0,
                    1,
                ),
            ],
            override_markdown: None,
        };

        let doc = make_doc(vec![
            make_page(0, "Page one body."),
            make_page(1, "Page two body."),
            make_page(2, "Page three body."),
        ]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();

        assert_eq!(result.markdown.matches("www.pdfa.org").count(), 1);
        assert!(result.markdown.contains("Page one body."));
        assert!(result.markdown.contains("Page two body."));
        assert!(result.markdown.contains("Page three body."));
    }

    #[test]
    fn scholarly_front_matter_drops_decorative_images_and_keeps_captioned_figure() {
        let page = Page {
            page_num: 0,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                make_block_at(
                    0,
                    "Attention Is All You Need",
                    BlockKind::Heading { level: 1 },
                    Bbox::new(60.0, 40.0, 520.0, 82.0),
                    24.0,
                    0,
                ),
                make_block_at(
                    1,
                    "Ashish Vaswani",
                    BlockKind::Paragraph,
                    Bbox::new(80.0, 104.0, 260.0, 124.0),
                    12.0,
                    1,
                ),
                make_block_at(
                    2,
                    "",
                    BlockKind::Image {
                        path: Some("images/logo.png".to_string()),
                    },
                    Bbox::new(470.0, 24.0, 540.0, 72.0),
                    12.0,
                    2,
                ),
                make_block_at(
                    3,
                    "Abstract",
                    BlockKind::Heading { level: 2 },
                    Bbox::new(70.0, 170.0, 190.0, 194.0),
                    14.0,
                    3,
                ),
                make_block_at(
                    4,
                    "The Transformer replaces recurrence with attention.",
                    BlockKind::Paragraph,
                    Bbox::new(60.0, 210.0, 520.0, 270.0),
                    11.0,
                    4,
                ),
                make_block_at(
                    5,
                    "",
                    BlockKind::Figure {
                        path: Some("images/figure.png".to_string()),
                        caption: Some("Figure 1: Model overview".to_string()),
                    },
                    Bbox::new(110.0, 300.0, 470.0, 520.0),
                    12.0,
                    5,
                ),
                make_block_at(
                    6,
                    "1. Introduction",
                    BlockKind::Paragraph,
                    Bbox::new(60.0, 560.0, 240.0, 582.0),
                    13.0,
                    6,
                ),
            ],
            override_markdown: None,
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();

        assert!(!result.markdown.contains("images/logo.png"));
        assert!(result.markdown.contains("images/figure.png"));
        assert!(result.markdown.contains("*Figure 1: Model overview*"));
    }
}
