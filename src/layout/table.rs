use crate::config::TableMode;
use crate::document::types::{Bbox, BlockKind, DetectedTable, RawTextBlock, RawWord, TableRender};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TableDetectionStrategy {
    /// Ruled-line grid detection. Not selected by the production classifier
    /// yet; exercised by unit tests. Kept for future parity with the
    /// reference detector.
    #[allow(dead_code)]
    LineGrid,
    TextAlignment,
    /// Explicit region-based detection. Same status as `LineGrid`.
    #[allow(dead_code)]
    ExplicitRegion,
}

fn page_number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*[-–—]?\s*\d+\s*[-–—]?\s*$|^\s*[Pp]age\s+\d+\s*$").unwrap())
}

fn ordered_list_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(\d+[.)]\s+|\(?[a-zA-Z][.)]\s+)").unwrap())
}

fn unordered_list_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*[•·▪▸►\-\*]\s+").unwrap())
}

fn caption_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^\s*(figure|fig\.?|table|tbl\.?|algorithm|listing|exhibit)\s+[\dIVXivx]+[.:)]",
        )
        .unwrap()
    })
}

fn affiliation_keyword_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(university|institute|department|school|college|laborator(?:y|ies)|research|openai|google|microsoft|meta|deepmind|anthropic|inc\.?|corp\.?|llc|san francisco|california|toronto|eth zurich)\b",
        )
        .unwrap()
    })
}

fn numeric_cell_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\s*\(?-?(?:\d{1,3}(?:[.,]\d{3})+(?:[.,]\d{2})?|\d+(?:[.,]\d{2})|\d+)\)?\s*$")
            .unwrap()
    })
}

fn numbered_section_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(\d+(?:\.\d+)*)(?:[.)])?\s+(.+\S)\s*$").unwrap())
}

fn partial_numbering_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\.\d+$").unwrap())
}

/// Detect table cells by finding blocks arranged in a 2D grid.
/// Uses a region-based text-alignment strategy: identifies candidate table
/// regions among non-heading/non-caption blocks, then validates each region.
pub(crate) fn detect_table_cells_with_font_size(
    blocks: &[RawTextBlock],
    body_font_size: f32,
) -> std::collections::HashMap<usize, BlockKind> {
    detect_table_cells_with_strategy(
        blocks,
        body_font_size,
        TableDetectionStrategy::TextAlignment,
    )
}

#[cfg(test)]
pub(crate) fn detect_table_cells(
    blocks: &[RawTextBlock],
) -> std::collections::HashMap<usize, BlockKind> {
    detect_table_cells_with_font_size(blocks, 0.0)
}

pub(crate) fn detect_table_cells_with_strategy(
    blocks: &[RawTextBlock],
    body_font_size: f32,
    strategy: TableDetectionStrategy,
) -> std::collections::HashMap<usize, BlockKind> {
    match strategy {
        TableDetectionStrategy::LineGrid | TableDetectionStrategy::ExplicitRegion => {
            detect_text_alignment_table_cells(blocks, body_font_size)
        }
        TableDetectionStrategy::TextAlignment => {
            detect_text_alignment_table_cells(blocks, body_font_size)
        }
    }
}

fn detect_text_alignment_table_cells(
    blocks: &[RawTextBlock],
    body_font_size: f32,
) -> std::collections::HashMap<usize, BlockKind> {
    use std::collections::HashMap;

    let mut result = HashMap::new();

    if blocks.len() < 4 {
        return result;
    }

    let candidate_blocks: Vec<&RawTextBlock> = blocks
        .iter()
        .filter(|b| !is_non_table_block(b, body_font_size))
        .collect();

    if candidate_blocks.len() < 4 {
        return result;
    }

    let regions = find_table_regions(&candidate_blocks);

    for region in &regions {
        if region.len() < 4 {
            continue;
        }

        for (block_id, kind) in detect_table_in_region(region) {
            result.insert(block_id, kind);
        }
    }

    result
}

fn is_non_table_block(block: &RawTextBlock, body_font_size: f32) -> bool {
    let text = block.text.trim();

    if text.is_empty() {
        return true;
    }

    if caption_re().is_match(text) {
        return true;
    }

    if page_number_re().is_match(text) && block.bbox.width() < 40.0 && text.len() <= 6 {
        return true;
    }

    if ordered_list_re().is_match(text) || unordered_list_re().is_match(text) {
        return true;
    }

    if looks_like_author_affiliation_block(text) {
        return true;
    }

    if body_font_size > 0.0 && block.font_size / body_font_size >= 1.15 {
        return true;
    }

    if body_font_size > 0.0 && looks_like_numbered_section_heading(text, block, body_font_size) {
        return true;
    }

    text.len() > 200
}

fn looks_like_numbered_section_heading(
    text: &str,
    block: &RawTextBlock,
    body_font_size: f32,
) -> bool {
    let Some(captures) = numbered_section_re().captures(text) else {
        return false;
    };
    let title = captures.get(2).map(|m| m.as_str().trim()).unwrap_or("");
    if title.is_empty() || title.len() > 100 || title.ends_with('.') {
        return false;
    }
    block.font_size / body_font_size.max(0.1) >= 1.05 || is_likely_heading_text(title)
}

fn is_likely_heading_text(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || words.len() > 12 {
        return false;
    }
    let cap_count = words
        .iter()
        .filter(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
        .count();
    cap_count as f32 / words.len() as f32 >= 0.6
}

fn looks_like_author_affiliation_block(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.contains('@') || affiliation_keyword_re().is_match(trimmed) {
        return true;
    }

    let non_empty_lines: Vec<&str> = trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if non_empty_lines.len() >= 2 {
        return looks_like_author_name_line(non_empty_lines[0]);
    }

    false
}

fn looks_like_author_name_line(text: &str) -> bool {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';'))
        .filter(|word| !word.is_empty())
        .collect();
    if !(2..=12).contains(&words.len()) {
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
                .find(|c| c.is_alphabetic())
                .map(|c| c.is_uppercase())
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
                    .find(|c| c.is_alphabetic())
                    .map(|c| c.is_lowercase())
                    .unwrap_or(false)
        })
        .count();

    capitalized * 5 >= words.len() * 4 && stopword_count == 0 && lowercase_content_words == 0
}

fn table_column_anchor(block: &RawTextBlock) -> f32 {
    if numeric_cell_re().is_match(block.text.trim()) {
        block.bbox.x1
    } else {
        block.bbox.x0
    }
}

fn find_table_regions<'a>(blocks: &[&'a RawTextBlock]) -> Vec<Vec<&'a RawTextBlock>> {
    if blocks.is_empty() {
        return vec![];
    }

    let mut sorted: Vec<&RawTextBlock> = blocks.to_vec();
    sorted.sort_by(|a, b| {
        a.bbox
            .y0
            .partial_cmp(&b.bbox.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let y_positions: Vec<f32> = sorted.iter().map(|b| b.bbox.y0).collect();
    let y_clusters = cluster_positions(&y_positions, 6.0);

    if y_clusters.len() < 2 {
        return vec![];
    }

    let mut row_assignments: Vec<(usize, &RawTextBlock)> = Vec::new();
    for &block in &sorted {
        if let Some(row) = nearest_cluster(block.bbox.y0, &y_clusters, 6.0) {
            row_assignments.push((row, block));
        }
    }

    let mut rows_map: std::collections::BTreeMap<usize, Vec<&RawTextBlock>> =
        std::collections::BTreeMap::new();
    for (row, block) in &row_assignments {
        rows_map.entry(*row).or_default().push(*block);
    }

    let table_rows: Vec<usize> = rows_map
        .iter()
        .filter(|(_, blocks_in_row)| blocks_in_row.len() >= 2)
        .map(|(row, _)| *row)
        .collect();

    if table_rows.len() < 2 {
        return vec![];
    }

    let mut regions: Vec<Vec<&'a RawTextBlock>> = Vec::new();
    let mut current_region: Vec<&RawTextBlock> = Vec::new();
    let mut prev_row: Option<usize> = None;

    for &row_idx in &table_rows {
        let is_contiguous = match prev_row {
            Some(prev) => row_idx <= prev + 2,
            None => true,
        };

        if is_contiguous {
            for &block in rows_map.get(&row_idx).unwrap() {
                current_region.push(block);
            }
        } else {
            if current_region.len() >= 4 {
                regions.push(current_region);
            }
            current_region = Vec::new();
            for &block in rows_map.get(&row_idx).unwrap() {
                current_region.push(block);
            }
        }
        prev_row = Some(row_idx);
    }
    if current_region.len() >= 4 {
        regions.push(current_region);
    }

    regions
}

fn detect_table_in_region(region: &[&RawTextBlock]) -> std::collections::HashMap<usize, BlockKind> {
    use std::collections::HashMap;

    let mut result = HashMap::new();

    let x_positions: Vec<f32> = region.iter().map(|b| table_column_anchor(b)).collect();
    let x_clusters = cluster_positions(&x_positions, 12.0);
    let y_positions: Vec<f32> = region.iter().map(|b| b.bbox.y0).collect();
    let y_clusters = cluster_positions(&y_positions, 6.0);

    if x_clusters.len() < 2 || y_clusters.len() < 2 || x_clusters.len() > 10 {
        return result;
    }

    for block in region.iter() {
        let col = nearest_cluster(table_column_anchor(block), &x_clusters, 12.0);
        let row = nearest_cluster(block.bbox.y0, &y_clusters, 6.0);
        if let (Some(col), Some(row)) = (col, row) {
            result.insert(block.block_id, BlockKind::TableCell { row, col });
        }
    }

    if result.len() < 4 {
        result.clear();
        return result;
    }

    let mut col_counts: HashMap<usize, usize> = HashMap::new();
    for kind in result.values() {
        if let BlockKind::TableCell { col, .. } = kind {
            *col_counts.entry(*col).or_insert(0) += 1;
        }
    }
    let cols_with_2_plus = col_counts.values().filter(|&&c| c >= 2).count();
    if cols_with_2_plus < 2 {
        result.clear();
        return result;
    }

    let assigned_blocks: Vec<&&RawTextBlock> = region
        .iter()
        .filter(|b| result.contains_key(&b.block_id))
        .collect();

    if let (Some(min_y), Some(max_y)) = (
        assigned_blocks.iter().map(|b| b.bbox.y0).reduce(f32::min),
        assigned_blocks.iter().map(|b| b.bbox.y1).reduce(f32::max),
    ) {
        let table_height = max_y - min_y;
        if table_height < 0.0 {
            result.clear();
        }
    }

    result
}

fn cluster_positions(positions: &[f32], tolerance: f32) -> Vec<f32> {
    let mut sorted = positions.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted.dedup_by(|a, b| (*b - *a).abs() < 0.1);

    let mut clusters: Vec<Vec<f32>> = Vec::new();
    for pos in sorted {
        if let Some(cluster) = clusters.last_mut() {
            let centre = cluster.iter().sum::<f32>() / cluster.len() as f32;
            if (pos - centre).abs() <= tolerance {
                cluster.push(pos);
                continue;
            }
        }
        clusters.push(vec![pos]);
    }

    clusters
        .iter()
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect()
}

fn nearest_cluster(value: f32, clusters: &[f32], tolerance: f32) -> Option<usize> {
    clusters
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (value - *a)
                .abs()
                .partial_cmp(&(value - *b).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|(i, &centre)| {
            if (value - centre).abs() <= tolerance {
                Some(i)
            } else {
                None
            }
        })
}

#[derive(Clone, Debug)]
pub(crate) struct TableCandidate {
    pub table: DetectedTable,
    pub source_block_ids: std::collections::BTreeSet<usize>,
    pub evidence: TableEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) enum TableEvidenceSource {
    RulingGrid,
    RulingBand,
    TextAlignment,
    NumericRows,
    ExplicitRegion,
    /// Reserved for an external table model backend. Not produced by any
    /// detection path yet; kept so evidence weights cover the full enum.
    #[allow(dead_code)]
    ExternalModel,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct TableEvidence {
    pub source: TableEvidenceSource,
    pub row_consistency: f32,
    pub column_alignment: f32,
    pub numeric_density: f32,
    pub row_count: usize,
    pub ruling_intersections: usize,
    pub caption_score: f32,
    pub broad_page_penalty: f32,
    pub prose_penalty: f32,
    pub debug_reasons: Vec<String>,
}

impl TableEvidence {
    pub(crate) fn score(&self) -> f32 {
        let source_bonus = match self.source {
            TableEvidenceSource::RulingGrid => 0.18,
            TableEvidenceSource::RulingBand => 0.12,
            TableEvidenceSource::NumericRows => 0.10,
            TableEvidenceSource::TextAlignment => 0.02,
            TableEvidenceSource::ExplicitRegion | TableEvidenceSource::ExternalModel => 0.14,
        };
        let row_count_bonus = match self.row_count {
            0..=3 => 0.0,
            4..=6 => 0.04,
            7..=9 => 0.08,
            _ => 0.12,
        };
        (self.row_consistency * 0.32
            + self.column_alignment * 0.24
            + self.numeric_density * 0.24
            + self.caption_score * 0.05
            + source_bonus
            + row_count_bonus
            - self.broad_page_penalty
            - self.prose_penalty)
            .clamp(0.0, 1.0)
    }

    pub(crate) fn has_independent_table_evidence(&self) -> bool {
        matches!(
            self.source,
            TableEvidenceSource::RulingGrid
                | TableEvidenceSource::RulingBand
                | TableEvidenceSource::NumericRows
                | TableEvidenceSource::ExplicitRegion
                | TableEvidenceSource::ExternalModel
        )
    }
}

impl TableCandidate {
    pub(crate) fn ranking_score(&self) -> f32 {
        let render_bonus = match self.table.render {
            TableRender::Markdown => 0.05,
            TableRender::Layout { .. } => 0.0,
        };
        self.table.confidence + render_bonus
    }

    pub(crate) fn is_broad_layout_candidate(&self, page_height: f32) -> bool {
        matches!(self.table.render, TableRender::Layout { .. })
            && self.table.bbox.height() >= page_height.max(1.0) * 0.35
    }

    pub(crate) fn should_emit(&self, page_width: f32, page_height: f32) -> bool {
        if matches!(self.table.render, TableRender::Layout { .. }) && self.table.confidence < 0.45 {
            return false;
        }
        if !self.is_broad_layout_candidate(page_height) {
            return true;
        }
        let broad_width = self.table.bbox.width() >= page_width.max(1.0) * 0.70;
        if !broad_width {
            return true;
        }
        self.evidence.has_independent_table_evidence()
            && self.table.confidence >= 0.72
            && self.evidence.prose_penalty < 0.12
    }
}

#[derive(Clone, Debug)]
struct WordRow<'a> {
    words: Vec<&'a RawWord>,
    bbox: Bbox,
    baseline_y: f32,
}

pub(crate) fn detect_coordinate_tables(
    words: &[RawWord],
    page_width: f32,
    mode: TableMode,
) -> Vec<TableCandidate> {
    if matches!(mode, TableMode::Off) || words.len() < 12 {
        return Vec::new();
    }

    let rows = build_word_rows(words);
    if rows.len() < 4 {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut idx = 0usize;
    while idx < rows.len() {
        if !looks_like_data_row(&rows[idx]) {
            idx += 1;
            continue;
        }

        let start = idx;
        let mut end = idx + 1;
        while end < rows.len()
            && (looks_like_data_row(&rows[end]) || weak_continuation_row(&rows[end]))
            && rows[end].baseline_y - rows[end - 1].baseline_y <= median_font_size(&rows[end]) * 2.6
        {
            end += 1;
        }

        let data_rows: Vec<usize> = (start..end)
            .filter(|row_idx| looks_like_data_row(&rows[*row_idx]))
            .collect();
        if data_rows.len() < 3 {
            idx = end;
            continue;
        }

        let header_start = find_header_start(&rows, start);
        let region_rows = &rows[header_start..end];
        if let Some(candidate) = build_table_candidate(
            region_rows,
            start - header_start,
            page_width,
            mode,
            TableEvidenceSource::NumericRows,
        ) {
            candidates.push(candidate);
        }
        idx = end;
    }

    if candidates.is_empty() {
        candidates.extend(detect_form_column_tables(&rows, page_width, mode));
        candidates.extend(detect_alignment_tables(&rows, page_width, mode));
        candidates.extend(detect_captioned_table_runs(&rows, page_width, mode));
    }

    suppress_overlapping_tables(candidates)
}

fn build_word_rows(words: &[RawWord]) -> Vec<WordRow<'_>> {
    let mut sorted: Vec<&RawWord> = words
        .iter()
        .filter(|word| !word.text.trim().is_empty() && word.bbox.width() > 0.0)
        .collect();
    sorted.sort_by(|left, right| {
        left.baseline_y
            .partial_cmp(&right.baseline_y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<WordRow<'_>> = Vec::new();
    for word in sorted {
        let tolerance = (word.font_size * 0.55).max(3.0);
        if let Some(row) = rows
            .iter_mut()
            .find(|row| (word.baseline_y - row.baseline_y).abs() <= tolerance)
        {
            row.baseline_y = (row.baseline_y * row.words.len() as f32 + word.baseline_y)
                / (row.words.len() as f32 + 1.0);
            row.bbox = row.bbox.union(&word.bbox);
            row.words.push(word);
            continue;
        }

        rows.push(WordRow {
            words: vec![word],
            bbox: word.bbox,
            baseline_y: word.baseline_y,
        });
    }

    for row in &mut rows {
        row.words.sort_by(|left, right| {
            left.bbox
                .x0
                .partial_cmp(&right.bbox.x0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    rows.sort_by(|left, right| {
        left.baseline_y
            .partial_cmp(&right.baseline_y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

fn looks_like_data_row(row: &WordRow<'_>) -> bool {
    if row.words.len() < 4 {
        return false;
    }
    let numeric = row
        .words
        .iter()
        .filter(|word| looks_like_table_value(&word.text))
        .count();
    let stock_like = row
        .words
        .iter()
        .filter(|word| word.text.chars().filter(|ch| ch.is_ascii_digit()).count() >= 6)
        .count();
    numeric * 100 / row.words.len().max(1) >= 55 || (stock_like >= 1 && numeric >= 4)
}

fn weak_continuation_row(row: &WordRow<'_>) -> bool {
    row.words.len() >= 4
        && row
            .words
            .iter()
            .filter(|word| looks_like_table_value(&word.text))
            .count()
            >= 3
}

fn looks_like_table_value(text: &str) -> bool {
    let trimmed = text.trim_matches(|ch: char| matches!(ch, ',' | ';' | ':' | '†' | '‡' | '*'));
    if trimmed.is_empty() {
        return false;
    }
    let digit_count = trimmed.chars().filter(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return matches!(trimmed, "-" | "–" | "—");
    }
    let allowed = trimmed.chars().all(|ch| {
        ch.is_ascii_digit()
            || ch.is_ascii_alphabetic()
            || matches!(
                ch,
                '.' | ',' | '/' | '-' | '–' | '—' | '"' | '\'' | '+' | '±'
            )
    });
    allowed && digit_count * 2 >= trimmed.chars().filter(|ch| !ch.is_whitespace()).count()
}

fn median_font_size(row: &WordRow<'_>) -> f32 {
    let mut sizes: Vec<f32> = row.words.iter().map(|word| word.font_size).collect();
    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sizes.get(sizes.len() / 2).copied().unwrap_or(10.0).max(1.0)
}

fn find_header_start(rows: &[WordRow<'_>], data_start: usize) -> usize {
    let mut header_start = data_start;
    let mut cursor = data_start;
    let mut included = 0usize;
    while cursor > 0 && included < 4 {
        let prev = cursor - 1;
        if rows[cursor].baseline_y - rows[prev].baseline_y > median_font_size(&rows[cursor]) * 2.8 {
            break;
        }
        if !looks_like_header_row(&rows[prev]) {
            break;
        }
        header_start = prev;
        cursor = prev;
        included += 1;
    }
    header_start
}

fn looks_like_header_row(row: &WordRow<'_>) -> bool {
    if row.words.is_empty() || row.words.len() > 24 {
        return false;
    }
    let text = row_text(row).to_ascii_lowercase();
    let keywords = [
        "nominal",
        "working",
        "load",
        "limit",
        "stock",
        "weight",
        "dimensions",
        "replacement",
        "size",
        "each",
        "(in)",
        "(kg)",
        "(mm)",
        "(t)",
        "tol",
        "pin",
        "bolt",
    ];
    keywords.iter().any(|keyword| text.contains(keyword))
}

#[derive(Clone, Debug)]
struct FormRowInfo {
    x_groups: Vec<f32>,
    is_table_row: bool,
    is_prose: bool,
    has_partial_numbering: bool,
}

fn detect_form_column_tables(
    rows: &[WordRow<'_>],
    page_width: f32,
    mode: TableMode,
) -> Vec<TableCandidate> {
    if rows.len() < 3 || page_width <= 0.0 {
        return Vec::new();
    }

    let mut row_info: Vec<FormRowInfo> = rows
        .iter()
        .map(|row| {
            let x_groups = form_row_x_groups(row, 45.0);
            let is_prose = looks_like_form_prose_row(row, page_width);
            let has_partial_numbering = row
                .words
                .first()
                .is_some_and(|word| partial_numbering_re().is_match(word.text.trim()));
            FormRowInfo {
                x_groups,
                is_table_row: false,
                is_prose,
                has_partial_numbering,
            }
        })
        .collect();

    let mut table_x_positions: Vec<f32> = row_info
        .iter()
        .filter(|info| info.x_groups.len() >= 3 && !info.is_prose && !info.has_partial_numbering)
        .flat_map(|info| info.x_groups.iter().copied())
        .collect();
    if table_x_positions.len() < 9 {
        return Vec::new();
    }

    let Some(columns) = form_global_columns(&mut table_x_positions, page_width) else {
        return Vec::new();
    };
    if columns.len() < 3 {
        return Vec::new();
    }

    for info in &mut row_info {
        if info.is_prose || info.has_partial_numbering {
            continue;
        }
        let aligned = form_aligned_column_count(&info.x_groups, &columns);
        info.is_table_row = aligned >= 2;
    }

    let table_row_count = row_info.iter().filter(|info| info.is_table_row).count();
    if table_row_count < 3 || table_row_count * 5 < row_info.len() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut idx = 0usize;
    while idx < rows.len() {
        if !row_info[idx].is_table_row {
            idx += 1;
            continue;
        }

        let start = idx;
        idx += 1;
        while idx < rows.len() && row_info[idx].is_table_row {
            let gap = rows[idx].baseline_y - rows[idx - 1].baseline_y;
            if gap > median_font_size(&rows[idx]).max(median_font_size(&rows[idx - 1])) * 3.4 {
                break;
            }
            idx += 1;
        }
        let end = idx;
        if end - start < 3 {
            continue;
        }
        if let Some(candidate) = build_form_column_table_candidate(
            &rows[start..end],
            &row_info[start..end],
            &columns,
            page_width,
            mode,
        ) {
            candidates.push(candidate);
        }
    }

    candidates
}

fn form_row_x_groups(row: &WordRow<'_>, gap_threshold: f32) -> Vec<f32> {
    let mut groups = Vec::new();
    for word in &row.words {
        let x = word.bbox.x0;
        if let Some(last) = groups.last_mut() {
            if x - *last <= gap_threshold {
                *last = (*last + x) / 2.0;
                continue;
            }
        }
        groups.push(x);
    }
    groups
}

fn looks_like_form_prose_row(row: &WordRow<'_>, page_width: f32) -> bool {
    let text = row_text(row);
    (row.bbox.width() > page_width * 0.55 && text.chars().count() > 60)
        || looks_like_prose_table_row(row)
}

fn form_global_columns(x_positions: &mut [f32], page_width: f32) -> Option<Vec<f32>> {
    x_positions.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));

    let mut gaps: Vec<f32> = x_positions
        .windows(2)
        .filter_map(|pair| {
            let gap = pair[1] - pair[0];
            (gap > 5.0).then_some(gap)
        })
        .collect();
    gaps.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let tolerance = if gaps.len() >= 3 {
        let idx = ((gaps.len() as f32) * 0.70).floor() as usize;
        gaps[idx.min(gaps.len() - 1)].clamp(25.0, 50.0)
    } else {
        35.0
    };

    let mut columns: Vec<f32> = Vec::new();
    let mut current: Vec<f32> = Vec::new();
    for x in x_positions.iter().copied() {
        if current.is_empty() {
            current.push(x);
            continue;
        }
        let mean = current.iter().sum::<f32>() / current.len() as f32;
        if (x - mean).abs() <= tolerance {
            current.push(x);
        } else {
            columns.push(current.iter().sum::<f32>() / current.len() as f32);
            current = vec![x];
        }
    }
    if !current.is_empty() {
        columns.push(current.iter().sum::<f32>() / current.len() as f32);
    }

    if columns.len() < 3 {
        return None;
    }
    if let (Some(first), Some(last)) = (columns.first(), columns.last()) {
        let content_width = last - first;
        if content_width <= 0.0 {
            return None;
        }
        let avg_col_width = content_width / columns.len() as f32;
        if avg_col_width < 30.0 {
            return None;
        }
        let columns_per_inch = columns.len() as f32 / (content_width / 72.0).max(0.1);
        if columns_per_inch > 10.0 {
            return None;
        }
    }
    let adaptive_max = ((20.0 * (page_width / 612.0)).round() as usize).max(15);
    if columns.len() > adaptive_max {
        return None;
    }

    Some(columns)
}

fn form_aligned_column_count(x_groups: &[f32], columns: &[f32]) -> usize {
    let mut aligned = std::collections::BTreeSet::new();
    for x in x_groups {
        if let Some((idx, distance)) = columns
            .iter()
            .enumerate()
            .map(|(idx, column)| (idx, (*x - *column).abs()))
            .min_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            if distance <= 40.0 {
                aligned.insert(idx);
            }
        }
    }
    aligned.len()
}

fn build_form_column_table_candidate(
    rows: &[WordRow<'_>],
    row_info: &[FormRowInfo],
    columns: &[f32],
    page_width: f32,
    mode: TableMode,
) -> Option<TableCandidate> {
    let bbox = rows
        .iter()
        .map(|row| row.bbox)
        .reduce(|bbox, row_bbox| bbox.union(&row_bbox))?;
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| assign_form_row_to_columns(row, columns))
        .collect();
    let layout_text = render_layout_text(rows);
    let evidence = form_table_evidence(rows, row_info, columns.len(), page_width);
    let confidence = evidence.score().max(0.50);
    let render = match mode {
        TableMode::Off => return None,
        TableMode::Native => TableRender::Markdown,
        TableMode::Layout => TableRender::Layout { text: layout_text },
        TableMode::Auto if confidence >= 0.68 && rows.len() >= 3 => TableRender::Markdown,
        TableMode::Auto => TableRender::Layout { text: layout_text },
    };
    let source_block_ids = rows
        .iter()
        .flat_map(|row| row.words.iter().map(|word| word.block_id))
        .collect();

    Some(TableCandidate {
        table: DetectedTable {
            bbox,
            rows: table_rows,
            confidence,
            render,
        },
        source_block_ids,
        evidence,
    })
}

fn assign_form_row_to_columns(row: &WordRow<'_>, columns: &[f32]) -> Vec<String> {
    let mut cells = vec![Vec::<String>::new(); columns.len()];
    for word in &row.words {
        let col = form_column_for_x(word.bbox.x0, columns);
        cells[col].push(word.text.trim().to_string());
    }
    cells.into_iter().map(|cell| cell.join(" ")).collect()
}

fn form_column_for_x(x: f32, columns: &[f32]) -> usize {
    if columns.len() <= 1 {
        return 0;
    }
    for idx in 0..columns.len() - 1 {
        if x < columns[idx + 1] - 20.0 {
            return idx;
        }
    }
    columns.len() - 1
}

fn form_table_evidence(
    rows: &[WordRow<'_>],
    row_info: &[FormRowInfo],
    column_count: usize,
    page_width: f32,
) -> TableEvidence {
    let table_rows = row_info.iter().filter(|info| info.is_table_row).count();
    let row_consistency = table_rows as f32 / rows.len().max(1) as f32;
    let numeric_density = rows
        .iter()
        .flat_map(|row| row.words.iter())
        .filter(|word| looks_like_table_value(&word.text))
        .count() as f32
        / rows.iter().map(|row| row.words.len()).sum::<usize>().max(1) as f32;
    let prose_rows = row_info.iter().filter(|info| info.is_prose).count();
    let width_ratio = rows
        .iter()
        .map(|row| row.bbox.width())
        .fold(0.0f32, f32::max)
        / page_width.max(1.0);
    let column_alignment = (row_consistency * 0.55
        + width_ratio.min(1.0) * 0.20
        + (column_count as f32 / 6.0).min(1.0) * 0.25)
        .clamp(0.0, 1.0);
    TableEvidence {
        source: TableEvidenceSource::TextAlignment,
        row_consistency,
        column_alignment,
        numeric_density,
        row_count: rows.len(),
        ruling_intersections: 0,
        caption_score: 0.0,
        broad_page_penalty: 0.0,
        prose_penalty: prose_rows as f32 / rows.len().max(1) as f32 * 0.30,
        debug_reasons: vec!["form-column-cluster".to_string()],
    }
}

fn detect_alignment_tables(
    rows: &[WordRow<'_>],
    page_width: f32,
    mode: TableMode,
) -> Vec<TableCandidate> {
    let mut candidates = Vec::new();
    let mut idx = 0usize;
    while idx < rows.len() {
        if !looks_like_alignment_row(&rows[idx]) {
            idx += 1;
            continue;
        }

        let start = idx;
        let mut end = idx + 1;
        while end < rows.len() && looks_like_alignment_row(&rows[end]) {
            let gap = rows[end].baseline_y - rows[end - 1].baseline_y;
            if gap > median_font_size(&rows[end]) * 2.8 {
                break;
            }
            end += 1;
        }

        let run = &rows[start..end];
        if run.len() >= 4 && !run.iter().all(|row| looks_like_prose_table_row(row)) {
            if let Some(candidate) = build_alignment_table_candidate(run, page_width, mode) {
                candidates.push(candidate);
            }
        }
        idx = end;
    }
    candidates
}

fn detect_captioned_table_runs(
    rows: &[WordRow<'_>],
    page_width: f32,
    mode: TableMode,
) -> Vec<TableCandidate> {
    let mut candidates = Vec::new();
    let mut idx = 0usize;
    while idx < rows.len() {
        if !looks_like_table_caption_row(&rows[idx]) {
            idx += 1;
            continue;
        }

        let start = idx + 1;
        let mut end = start;
        while end < rows.len() {
            if end > start {
                let gap = rows[end].baseline_y - rows[end - 1].baseline_y;
                if gap > median_font_size(&rows[end]) * 3.4 {
                    break;
                }
            }
            if end > start && looks_like_table_caption_row(&rows[end]) {
                break;
            }
            if end > start + 2 && looks_like_post_table_section_break(&rows[end]) {
                break;
            }
            if end > start + 3 && looks_like_post_table_sentence(&rows[end]) {
                break;
            }
            if end > start + 3 && looks_like_prose_table_row(&rows[end]) {
                break;
            }
            end += 1;
            if end - start >= 18 {
                break;
            }
        }

        if end.saturating_sub(start) >= 3 {
            if let Some(candidate) =
                build_captioned_table_candidate(&rows[start..end], page_width, mode)
            {
                candidates.push(candidate);
            }
        }
        idx = end.max(idx + 1);
    }
    candidates
}

fn looks_like_table_caption_row(row: &WordRow<'_>) -> bool {
    let text = row_text(row).to_ascii_lowercase();
    text.contains("table ") && text.contains(':')
}

fn looks_like_post_table_section_break(row: &WordRow<'_>) -> bool {
    let text = row_text(row);
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    let Some(second) = words.next() else {
        return false;
    };
    first
        .trim_matches(|ch: char| matches!(ch, '.' | ')' | '('))
        .chars()
        .all(|ch| ch.is_ascii_digit())
        && second
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn looks_like_post_table_sentence(row: &WordRow<'_>) -> bool {
    let text = row_text(row);
    let lower = text.to_ascii_lowercase();
    row.words.len() >= 7
        && text.trim_end().ends_with('.')
        && ["these ", "this ", "the ", "values "]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
}

fn build_captioned_table_candidate(
    rows: &[WordRow<'_>],
    page_width: f32,
    mode: TableMode,
) -> Option<TableCandidate> {
    let bbox = rows
        .iter()
        .map(|row| row.bbox)
        .reduce(|bbox, row_bbox| bbox.union(&row_bbox))?;
    let markdown_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.words.iter().map(|word| word.text.clone()).collect())
        .collect();
    let numeric_density = rows
        .iter()
        .flat_map(|row| row.words.iter())
        .filter(|word| looks_like_table_value(&word.text))
        .count() as f32
        / rows.iter().map(|row| row.words.len()).sum::<usize>().max(1) as f32;
    let row_consistency = word_row_width_consistency(rows);
    let width_ratio = rows
        .iter()
        .map(|row| row.bbox.width())
        .fold(0.0f32, f32::max)
        / page_width.max(1.0);
    let evidence = TableEvidence {
        source: TableEvidenceSource::ExplicitRegion,
        row_consistency,
        column_alignment: (row_consistency * 0.65 + width_ratio.min(1.0) * 0.35).clamp(0.0, 1.0),
        numeric_density,
        row_count: rows.len(),
        ruling_intersections: 0,
        caption_score: 0.95,
        broad_page_penalty: 0.0,
        prose_penalty: 0.0,
        debug_reasons: vec!["captioned-table-run".to_string()],
    };
    let confidence = evidence.score().max(0.48);
    let layout_text = render_layout_text(rows);
    let render = match mode {
        TableMode::Off => return None,
        TableMode::Native => TableRender::Markdown,
        TableMode::Layout | TableMode::Auto => TableRender::Layout { text: layout_text },
    };
    let source_block_ids = rows
        .iter()
        .flat_map(|row| row.words.iter().map(|word| word.block_id))
        .collect();

    Some(TableCandidate {
        table: DetectedTable {
            bbox,
            rows: markdown_rows,
            confidence,
            render,
        },
        source_block_ids,
        evidence,
    })
}

fn word_row_width_consistency(rows: &[WordRow<'_>]) -> f32 {
    let mut counts = std::collections::BTreeMap::new();
    for row in rows {
        *counts.entry(row.words.len()).or_insert(0usize) += 1;
    }
    counts
        .values()
        .copied()
        .max()
        .map(|max| max as f32 / rows.len().max(1) as f32)
        .unwrap_or(0.0)
}

fn looks_like_alignment_row(row: &WordRow<'_>) -> bool {
    if row.words.len() < 3 || row.words.len() > 16 {
        return false;
    }
    let width = row.bbox.width();
    width >= 70.0
        && row
            .words
            .iter()
            .filter(|word| !word.text.trim().is_empty())
            .count()
            >= 3
}

fn looks_like_prose_table_row(row: &WordRow<'_>) -> bool {
    if row.words.len() < 6 {
        return false;
    }
    let stopwords = [
        "a", "an", "and", "are", "as", "at", "for", "from", "in", "is", "of", "on", "or", "the",
        "to", "with", "without",
    ];
    let stopword_count = row
        .words
        .iter()
        .filter(|word| stopwords.contains(&word.text.to_ascii_lowercase().as_str()))
        .count();
    let numeric_count = row
        .words
        .iter()
        .filter(|word| looks_like_table_value(&word.text))
        .count();
    stopword_count >= 1 && numeric_count == 0
}

fn build_alignment_table_candidate(
    rows: &[WordRow<'_>],
    page_width: f32,
    mode: TableMode,
) -> Option<TableCandidate> {
    let mut count_histogram = std::collections::BTreeMap::new();
    for row in rows {
        *count_histogram.entry(row.words.len()).or_insert(0usize) += 1;
    }
    let (column_count, matching_rows) = count_histogram
        .into_iter()
        .max_by_key(|(cols, hits)| (*hits, *cols))?;
    if column_count < 3 || matching_rows < 3 {
        return None;
    }

    let matching: Vec<&WordRow<'_>> = rows
        .iter()
        .filter(|row| row.words.len() == column_count)
        .collect();
    let centers = average_column_centers(&matching, column_count)?;
    if !columns_are_stable(&matching, &centers) {
        return None;
    }

    build_table_candidate(
        rows,
        1.min(rows.len() - 1),
        page_width,
        mode,
        TableEvidenceSource::TextAlignment,
    )
}

fn average_column_centers(rows: &[&WordRow<'_>], column_count: usize) -> Option<Vec<f32>> {
    let mut centers = vec![0.0; column_count];
    for row in rows {
        for (idx, word) in row.words.iter().enumerate() {
            centers[idx] += word.bbox.center_x();
        }
    }
    for center in &mut centers {
        *center /= rows.len().max(1) as f32;
    }
    Some(centers)
}

fn columns_are_stable(rows: &[&WordRow<'_>], centers: &[f32]) -> bool {
    rows.iter().all(|row| {
        row.words.iter().zip(centers.iter()).all(|(word, center)| {
            (word.bbox.center_x() - center).abs() <= word.font_size.max(8.0) * 1.8
        })
    })
}

fn build_table_candidate(
    rows: &[WordRow<'_>],
    first_data_idx: usize,
    page_width: f32,
    mode: TableMode,
    evidence_source: TableEvidenceSource,
) -> Option<TableCandidate> {
    let data_rows = &rows[first_data_idx..];
    let mut count_histogram = std::collections::BTreeMap::new();
    for row in data_rows {
        *count_histogram.entry(row.words.len()).or_insert(0usize) += 1;
    }
    let (column_count, matching_rows) = count_histogram
        .into_iter()
        .max_by_key(|(cols, hits)| (*hits, *cols))?;
    if column_count < 3 || matching_rows < 3 {
        return None;
    }
    if prose_page_like_table_run(data_rows) {
        return None;
    }

    let mut centers = vec![0.0; column_count];
    let mut counts = vec![0usize; column_count];
    for row in data_rows
        .iter()
        .filter(|row| row.words.len() == column_count)
    {
        for (idx, word) in row.words.iter().enumerate() {
            centers[idx] += word.bbox.center_x();
            counts[idx] += 1;
        }
    }
    for (center, count) in centers.iter_mut().zip(counts.iter()) {
        if *count == 0 {
            return None;
        }
        *center /= *count as f32;
    }

    let header = build_header(rows, first_data_idx, &centers);
    let mut table_rows = Vec::new();
    table_rows.push(header);
    for row in data_rows {
        table_rows.push(assign_row_to_columns(row, &centers));
    }

    let evidence = table_evidence(
        data_rows,
        column_count,
        matching_rows,
        page_width,
        evidence_source,
    );
    let confidence = evidence.score();
    let bbox = rows
        .iter()
        .map(|row| row.bbox)
        .reduce(|bbox, row_bbox| bbox.union(&row_bbox))?;
    let layout_text = render_layout_text(rows);
    let has_irregular_rows = data_rows.iter().any(|row| row.words.len() != column_count);
    let render = match mode {
        TableMode::Layout => TableRender::Layout { text: layout_text },
        TableMode::Native => TableRender::Markdown,
        TableMode::Auto if confidence >= 0.70 && !has_irregular_rows => TableRender::Markdown,
        TableMode::Auto => TableRender::Layout { text: layout_text },
        TableMode::Off => return None,
    };

    let source_block_ids = rows
        .iter()
        .flat_map(|row| row.words.iter().map(|word| word.block_id))
        .collect();

    Some(TableCandidate {
        table: DetectedTable {
            bbox,
            rows: table_rows,
            confidence,
            render,
        },
        source_block_ids,
        evidence,
    })
}

fn prose_page_like_table_run(rows: &[WordRow<'_>]) -> bool {
    if rows.len() < 8 {
        return false;
    }
    let prose_rows = rows
        .iter()
        .filter(|row| looks_like_prose_table_row(row))
        .count();
    prose_rows * 10 >= rows.len() * 3
}

fn build_header(rows: &[WordRow<'_>], first_data_idx: usize, centers: &[f32]) -> Vec<String> {
    let mut cells = vec![Vec::<String>::new(); centers.len()];
    for row in &rows[..first_data_idx] {
        for word in &row.words {
            if let Some(col) = nearest_center(word.bbox.center_x(), centers) {
                cells[col].push(clean_header_token(&word.text));
            }
        }
    }

    cells
        .into_iter()
        .enumerate()
        .map(|(idx, tokens)| {
            let text = collapse_header_tokens(&tokens);
            if text.is_empty() {
                format!("Column {}", idx + 1)
            } else {
                text
            }
        })
        .collect()
}

fn clean_header_token(token: &str) -> String {
    token.trim().trim_matches('|').to_string()
}

fn collapse_header_tokens(tokens: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for token in tokens {
        let trimmed = token.trim();
        if trimmed.is_empty() || out.iter().any(|existing| existing == trimmed) {
            continue;
        }
        out.push(trimmed.to_string());
    }
    out.join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn assign_row_to_columns(row: &WordRow<'_>, centers: &[f32]) -> Vec<String> {
    if row.words.len() == centers.len() {
        return row.words.iter().map(|word| word.text.clone()).collect();
    }

    let mut cells = vec![Vec::<String>::new(); centers.len()];
    for word in &row.words {
        if let Some(col) = nearest_center(word.bbox.center_x(), centers) {
            cells[col].push(word.text.clone());
        }
    }
    cells.into_iter().map(|cell| cell.join(" ")).collect()
}

fn nearest_center(value: f32, centers: &[f32]) -> Option<usize> {
    centers
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            (value - **left)
                .abs()
                .partial_cmp(&(value - **right).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(idx, _)| idx)
}

fn table_evidence(
    data_rows: &[WordRow<'_>],
    column_count: usize,
    matching_rows: usize,
    page_width: f32,
    evidence_source: TableEvidenceSource,
) -> TableEvidence {
    let row_consistency = matching_rows as f32 / data_rows.len().max(1) as f32;
    let numeric_density = data_rows
        .iter()
        .flat_map(|row| row.words.iter())
        .filter(|word| looks_like_table_value(&word.text))
        .count() as f32
        / data_rows
            .iter()
            .map(|row| row.words.len())
            .sum::<usize>()
            .max(1) as f32;
    let width_ratio = data_rows
        .iter()
        .map(|row| row.bbox.width())
        .fold(0.0f32, f32::max)
        / page_width.max(1.0);
    let mut column_alignment = (row_consistency * 0.70 + width_ratio.min(1.0) * 0.30)
        + (column_count >= 6) as u8 as f32 * 0.05;
    if data_rows.iter().any(|row| row.words.len() != column_count) {
        column_alignment = column_alignment.min(0.68);
    }
    let prose_rows = data_rows
        .iter()
        .filter(|row| looks_like_prose_table_row(row))
        .count();
    let prose_penalty = prose_rows as f32 / data_rows.len().max(1) as f32 * 0.25;
    TableEvidence {
        source: evidence_source,
        row_consistency,
        column_alignment: column_alignment.clamp(0.0, 1.0),
        numeric_density,
        row_count: data_rows.len(),
        ruling_intersections: 0,
        caption_score: 0.0,
        broad_page_penalty: 0.0,
        prose_penalty,
        debug_reasons: vec![format!("{evidence_source:?}")],
    }
}

fn render_layout_text(rows: &[WordRow<'_>]) -> String {
    let min_x = rows
        .iter()
        .map(|row| row.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    let font_size = rows.iter().map(median_font_size).sum::<f32>() / rows.len().max(1) as f32;
    let points_per_char = (font_size * 0.45).max(3.5);

    let mut lines = Vec::new();
    for row in rows {
        let mut line = String::new();
        for word in &row.words {
            let target = ((word.bbox.x0 - min_x) / points_per_char).round().max(0.0) as usize;
            if line.len() < target {
                let gap = target - line.len();
                if gap <= 6 && !line.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(&" ".repeat(gap));
                }
            } else if !line.ends_with(' ') && !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word.text.trim());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn suppress_overlapping_tables(candidates: Vec<TableCandidate>) -> Vec<TableCandidate> {
    let mut kept: Vec<TableCandidate> = Vec::new();
    for candidate in candidates {
        let mut replacement = Some(candidate);
        for existing in &mut kept {
            let candidate_ref = replacement.as_ref().expect("candidate still available");
            if overlap_ratio(candidate_ref.table.bbox, existing.table.bbox) > 0.65 {
                if table_candidate_cmp(candidate_ref, existing).is_gt() {
                    *existing = replacement.take().expect("candidate still available");
                }
                break;
            }
        }
        if let Some(candidate) = replacement {
            kept.push(candidate);
        }
    }
    kept.sort_by(|left, right| {
        left.table
            .bbox
            .y0
            .partial_cmp(&right.table.bbox.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.table
                    .bbox
                    .x0
                    .partial_cmp(&right.table.bbox.x0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    kept
}

fn table_candidate_cmp(left: &TableCandidate, right: &TableCandidate) -> std::cmp::Ordering {
    left.ranking_score()
        .partial_cmp(&right.ranking_score())
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| {
            right
                .table
                .bbox
                .area()
                .partial_cmp(&left.table.bbox.area())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn overlap_ratio(a: Bbox, b: Bbox) -> f32 {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    let intersection = ((x1 - x0).max(0.0)) * ((y1 - y0).max(0.0));
    let smaller = a.area().min(b.area()).max(1.0);
    intersection / smaller
}

fn row_text(row: &WordRow<'_>) -> String {
    row.words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::types::Bbox;

    fn make_block_id(
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        text: &str,
        block_id: usize,
    ) -> RawTextBlock {
        RawTextBlock {
            bbox: Bbox::new(x0, y0, x1, y1),
            text: text.to_string(),
            font_size: 10.0,
            font_name: "unknown".to_string(),
            page_num: 0,
            block_id,
            reading_order: 0,
        }
    }

    fn word(text: &str, col: usize, row: usize) -> RawWord {
        let x0 = 50.0 + col as f32 * 42.0;
        let y0 = 100.0 + row as f32 * 14.0;
        RawWord {
            bbox: Bbox::new(x0, y0, x0 + 24.0, y0 + 10.0),
            text: text.to_string(),
            font_size: 9.0,
            page_num: 0,
            block_id: row,
            line_id: row,
            baseline_y: y0 + 8.0,
        }
    }

    fn row_words(row: usize, values: &[&str]) -> Vec<RawWord> {
        values
            .iter()
            .enumerate()
            .map(|(col, value)| word(value, col, row))
            .collect()
    }

    fn word_at(text: &str, x0: f32, row: usize, width: f32) -> RawWord {
        let y0 = 100.0 + row as f32 * 14.0;
        RawWord {
            bbox: Bbox::new(x0, y0, x0 + width, y0 + 10.0),
            text: text.to_string(),
            font_size: 9.0,
            page_num: 0,
            block_id: row,
            line_id: row,
            baseline_y: y0 + 8.0,
        }
    }

    #[test]
    fn text_alignment_strategy_detects_simple_grid() {
        let blocks = vec![
            make_block_id(50.0, 100.0, 150.0, 120.0, "A1", 0),
            make_block_id(200.0, 100.0, 300.0, 120.0, "A2", 1),
            make_block_id(50.0, 130.0, 150.0, 150.0, "B1", 2),
            make_block_id(200.0, 130.0, 300.0, 150.0, "B2", 3),
        ];

        let cells =
            detect_table_cells_with_strategy(&blocks, 10.0, TableDetectionStrategy::TextAlignment);

        assert_eq!(cells.len(), 4);
        assert!(cells
            .values()
            .all(|kind| matches!(kind, BlockKind::TableCell { .. })));
    }

    #[test]
    fn strategy_placeholders_preserve_current_behavior() {
        let blocks = vec![
            make_block_id(50.0, 100.0, 150.0, 120.0, "A1", 0),
            make_block_id(200.0, 100.0, 300.0, 120.0, "A2", 1),
            make_block_id(50.0, 130.0, 150.0, 150.0, "B1", 2),
            make_block_id(200.0, 130.0, 300.0, 150.0, "B2", 3),
        ];

        let line_grid =
            detect_table_cells_with_strategy(&blocks, 10.0, TableDetectionStrategy::LineGrid);
        let explicit =
            detect_table_cells_with_strategy(&blocks, 10.0, TableDetectionStrategy::ExplicitRegion);

        assert_eq!(line_grid.len(), 4);
        assert_eq!(explicit.len(), 4);
    }

    #[test]
    fn detects_wide_coordinate_table_from_words() {
        let mut words = Vec::new();
        words.extend(row_words(
            0,
            &[
                "Nominal", "Working", "Stock", "Weight", "A", "B", "C", "D", "E", "F", "G", "H",
            ],
        ));
        for row in 1..=4 {
            words.extend(row_words(
                row,
                &[
                    "1/4", "1/2", "1018017", ".06", "12", "1.5", "8", "29", "6", "20", "16", "-",
                ],
            ));
        }

        let tables = detect_coordinate_tables(&words, 600.0, TableMode::Native);

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].table.rows[0].len(), 12);
        assert!(matches!(tables[0].table.render, TableRender::Markdown));
    }

    #[test]
    fn coordinate_table_layout_mode_preserves_fixed_width_text() {
        let mut words = Vec::new();
        words.extend(row_words(0, &["Nominal", "Working", "Stock", "Weight"]));
        for row in 1..=3 {
            words.extend(row_words(row, &["1/4", "1/2", "1018017", ".06"]));
        }

        let tables = detect_coordinate_tables(&words, 600.0, TableMode::Layout);

        assert_eq!(tables.len(), 1);
        match &tables[0].table.render {
            TableRender::Layout { text } => {
                assert!(text.contains("Nominal"));
                assert!(text.contains("1018017"));
            }
            other => panic!("expected layout table, got {other:?}"),
        }
    }

    #[test]
    fn coordinate_table_ignores_normal_paragraph_words() {
        let mut words = Vec::new();
        for row in 0..5 {
            words.extend(row_words(
                row,
                &[
                    "This",
                    "is",
                    "ordinary",
                    "paragraph",
                    "text",
                    "without",
                    "data",
                ],
            ));
        }

        let tables = detect_coordinate_tables(&words, 600.0, TableMode::Auto);

        assert!(tables.is_empty());
    }

    #[test]
    fn coordinate_table_detects_text_alignment_grid() {
        let mut words = Vec::new();
        words.extend(row_words(0, &["Component", "Type", "Status"]));
        words.extend(row_words(1, &["Valve", "Gate", "Open"]));
        words.extend(row_words(2, &["Pump", "Centrifugal", "Running"]));
        words.extend(row_words(3, &["Motor", "Electric", "Spare"]));

        let tables = detect_coordinate_tables(&words, 600.0, TableMode::Auto);

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].table.rows[0], vec!["Component", "Type", "Status"]);
        assert_eq!(tables[0].table.rows[2][1], "Centrifugal");
    }

    #[test]
    fn coordinate_table_keeps_wrapped_label_rows_in_layout_fallback() {
        let mut words = Vec::new();
        words.extend(vec![
            word_at("Item", 50.0, 0, 24.0),
            word_at("2025", 200.0, 0, 26.0),
            word_at("2024", 300.0, 0, 26.0),
            word_at("2023", 400.0, 0, 26.0),
            word_at("Revenue", 50.0, 1, 46.0),
            word_at("100", 200.0, 1, 20.0),
            word_at("90", 300.0, 1, 16.0),
            word_at("80", 400.0, 1, 16.0),
            word_at("Assets", 50.0, 2, 38.0),
            word_at("70", 200.0, 2, 16.0),
            word_at("65", 300.0, 2, 16.0),
            word_at("60", 400.0, 2, 16.0),
            word_at("Equity", 50.0, 3, 34.0),
            word_at("30", 200.0, 3, 16.0),
            word_at("25", 300.0, 3, 16.0),
            word_at("20", 400.0, 3, 16.0),
            word_at("Operating", 50.0, 4, 54.0),
            word_at("profit", 108.0, 4, 32.0),
            word_at("50", 200.0, 4, 16.0),
            word_at("45", 300.0, 4, 16.0),
            word_at("40", 400.0, 4, 16.0),
        ]);

        let tables = detect_coordinate_tables(&words, 600.0, TableMode::Auto);

        assert_eq!(tables.len(), 1);
        match &tables[0].table.render {
            TableRender::Layout { text } => {
                assert!(text.contains("Operating profit"));
                assert!(text.contains("2025"));
            }
            other => panic!("expected ambiguous wrapped row to use layout fallback, got {other:?}"),
        }
    }

    #[test]
    fn form_column_table_preserves_blank_cells_and_multiword_headers() {
        let mut words = vec![
            word_at("Product", 50.0, 0, 42.0),
            word_at("Code", 94.0, 0, 28.0),
            word_at("Location", 150.0, 0, 48.0),
            word_at("Expected", 250.0, 0, 50.0),
            word_at("Actual", 350.0, 0, 36.0),
            word_at("Variance", 450.0, 0, 50.0),
            word_at("Status", 540.0, 0, 38.0),
            word_at("SKU-8847", 50.0, 1, 58.0),
            word_at("A-12", 150.0, 1, 28.0),
            word_at("450", 250.0, 1, 24.0),
            word_at("B-07", 150.0, 2, 28.0),
            word_at("289", 350.0, 2, 24.0),
            word_at("-23", 450.0, 2, 24.0),
            word_at("SKU-9201", 50.0, 3, 58.0),
            word_at("780", 250.0, 3, 24.0),
            word_at("778", 350.0, 3, 24.0),
            word_at("OK", 540.0, 3, 18.0),
        ];
        for (idx, word) in words.iter_mut().enumerate() {
            word.block_id = idx;
        }

        let tables = detect_coordinate_tables(&words, 620.0, TableMode::Auto);

        assert_eq!(tables.len(), 1);
        assert!(matches!(tables[0].table.render, TableRender::Markdown));
        assert_eq!(
            tables[0].table.rows[0],
            vec![
                "Product Code",
                "Location",
                "Expected",
                "Actual",
                "Variance",
                "Status"
            ]
        );
        assert_eq!(
            tables[0].table.rows[2],
            vec!["", "B-07", "", "289", "-23", ""]
        );
        assert!(tables[0]
            .evidence
            .debug_reasons
            .contains(&"form-column-cluster".to_string()));
    }

    #[test]
    fn form_column_table_layout_mode_stays_review_safe() {
        let mut words = Vec::new();
        words.extend(row_words(0, &["Code", "Location", "Expected"]));
        words.extend(row_words(1, &["A", "North", "10"]));
        words.extend(row_words(2, &["B", "South", "12"]));
        words.extend(row_words(3, &["C", "East", "14"]));

        let tables = detect_coordinate_tables(&words, 600.0, TableMode::Layout);

        assert_eq!(tables.len(), 1);
        assert!(matches!(tables[0].table.render, TableRender::Layout { .. }));
    }

    #[test]
    fn form_column_table_rejects_masterformat_partial_numbering() {
        let words = vec![
            word_at(".1", 50.0, 0, 14.0),
            word_at("The", 110.0, 0, 20.0),
            word_at("intent", 180.0, 0, 36.0),
            word_at(".2", 50.0, 1, 14.0),
            word_at("Available", 110.0, 1, 54.0),
            word_at("information", 180.0, 1, 70.0),
            word_at(".3", 50.0, 2, 14.0),
            word_at("Submit", 110.0, 2, 40.0),
            word_at("documents", 180.0, 2, 64.0),
        ];

        let tables = detect_coordinate_tables(&words, 600.0, TableMode::Auto);

        assert!(tables.is_empty());
    }
}
