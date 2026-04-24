use crate::document::types::{Block, BlockKind, RawPage, RawTextBlock};
use crate::pdf::metadata::PageMetadata;
use regex::Regex;
use std::sync::OnceLock;

// --- Regex patterns (compiled once) ---

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

fn code_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Heuristic: starts with common code patterns
    RE.get_or_init(|| {
        Regex::new(r"^\s*(```|~~~|def |fn |pub |class |import |from |#include|int |void |return )")
            .unwrap()
    })
}

fn scholarly_metadata_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^\s*(arxiv:\S+|doi:\S+|https?://doi\.org/\S+|preprint\b|accepted at\b|published as\b)",
        )
        .unwrap()
    })
}

fn scholarly_note_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(permission to make digital|all rights reserved|copyright held by|acm isbn|provided proper attribution|correspondence to:)",
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

fn struct_role_to_heading_level(role: &str) -> Option<u8> {
    match role {
        "H1" | "Title" => Some(1),
        "H2" => Some(2),
        "H3" => Some(3),
        "H4" => Some(4),
        "H5" => Some(5),
        "H6" => Some(6),
        _ => None,
    }
}

pub struct ClassifierConfig {
    /// Body font size (mode across document). Used as baseline for heading detection.
    pub body_font_size: f32,
    /// Font size >= body * this ratio is a heading candidate. Default: 1.15
    pub heading_size_ratio: f32,
    /// Top/bottom fraction of page height considered header/footer zone. Default: 0.07
    pub header_footer_zone: f32,
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            body_font_size: 10.0,
            heading_size_ratio: 1.15,
            header_footer_zone: 0.07,
        }
    }
}

pub struct Classifier {
    config: ClassifierConfig,
}

impl Classifier {
    /// Create a classifier with body font size computed from the document's pages.
    pub fn new_for_document(raw_pages: &[RawPage]) -> Self {
        let body_font_size = compute_body_font_size(raw_pages);
        Self {
            config: ClassifierConfig {
                body_font_size,
                ..Default::default()
            },
        }
    }

    #[allow(dead_code)]
    pub fn with_config(config: ClassifierConfig) -> Self {
        Self { config }
    }

    /// Classify all blocks on a page, returning `Block`s with `BlockKind` assigned.
    #[allow(dead_code)]
    pub fn classify_page(&self, raw_blocks: Vec<RawTextBlock>, page: &RawPage) -> Vec<Block> {
        self.classify_page_with_metadata(raw_blocks, page, None)
    }

    /// Classify a page with an optional `PageMetadata` sidecar providing
    /// font-weight and struct-tree signals. When `metadata` is `None`, this
    /// is equivalent to [`classify_page`].
    pub fn classify_page_with_metadata(
        &self,
        raw_blocks: Vec<RawTextBlock>,
        page: &RawPage,
        metadata: Option<&PageMetadata>,
    ) -> Vec<Block> {
        // First pass: detect table cells (pass body font size for heading exclusion)
        let table_cells =
            detect_table_cells_with_font_size(&raw_blocks, self.config.body_font_size);

        raw_blocks
            .into_iter()
            .map(|rb| {
                let kind = if let Some(tc) = table_cells.get(&rb.block_id) {
                    tc.clone()
                } else {
                    self.classify_block_with_metadata(&rb, page, metadata)
                };
                Block {
                    id: rb.block_id,
                    bbox: rb.bbox,
                    text: rb.text.clone(),
                    kind,
                    font_size: rb.font_size,
                    font_name: rb.font_name.clone(),
                    page_num: rb.page_num,
                    reading_order: rb.reading_order,
                }
            })
            .collect()
    }

    #[allow(dead_code)]
    fn classify_block(&self, block: &RawTextBlock, page: &RawPage) -> BlockKind {
        self.classify_block_with_metadata(block, page, None)
    }

    fn classify_block_with_metadata(
        &self,
        block: &RawTextBlock,
        page: &RawPage,
        metadata: Option<&PageMetadata>,
    ) -> BlockKind {
        let text = block.text.trim();

        if text.is_empty() {
            return BlockKind::Paragraph; // treat empty as paragraph, will be filtered by renderer
        }

        // Header/footer zone detection
        if self.is_in_header_zone(block, page) {
            if page_number_re().is_match(text) {
                return BlockKind::PageNumber;
            }
            return BlockKind::RunningHeader;
        }
        if self.is_in_footer_zone(block, page) {
            if page_number_re().is_match(text) {
                return BlockKind::PageNumber;
            }
            return BlockKind::RunningFooter;
        }

        // Page number (anywhere on page)
        if page_number_re().is_match(text) {
            return BlockKind::PageNumber;
        }

        // Caption
        if caption_re().is_match(text) {
            return BlockKind::Caption;
        }

        // Code block
        if code_block_re().is_match(text) {
            return BlockKind::CodeBlock;
        }

        // List items
        if ordered_list_re().is_match(text) {
            let depth = indent_depth(block, page);
            return BlockKind::ListItem {
                ordered: true,
                depth,
            };
        }
        if unordered_list_re().is_match(text) {
            let depth = indent_depth(block, page);
            return BlockKind::ListItem {
                ordered: false,
                depth,
            };
        }

        // Scholarly metadata and permission/copyright notes frequently use
        // large fonts on page 1, but should not be promoted to headings.
        if scholarly_metadata_re().is_match(text) || scholarly_note_re().is_match(text) {
            return BlockKind::Paragraph;
        }

        // Heading detection — prefer struct-tree role, then font-size ratio,
        // then bold-at-body-size as a last resort when metadata is present.
        if let Some(md) = metadata {
            if let Some(role) = md.struct_role_for_bbox(&block.bbox) {
                if let Some(level) = struct_role_to_heading_level(role) {
                    return BlockKind::Heading { level };
                }
            }
        }

        let ratio = block.font_size / self.config.body_font_size;
        if ratio >= self.config.heading_size_ratio {
            let level = self.font_size_to_heading_level(block.font_size);
            return BlockKind::Heading { level };
        }

        // Bold-at-body-size heading signal — only when metadata is available.
        // A short, bold, non-sentence-terminated line at body font size is
        // almost always a subsection heading in documents that don't use
        // size hierarchy.
        if let Some(md) = metadata {
            if let Some(font) = md.font_for_bbox(&block.bbox) {
                if font.is_bold() && text.len() <= 120 && !text.ends_with('.') {
                    return BlockKind::Heading { level: 4 };
                }
            }
        }

        // Short, single-line text at larger-ish size (section headers with same body size)
        // Heuristic: <= 80 chars, no trailing period, all-caps or title-case dominant
        // Only apply if we couldn't detect via size — weak signal, be conservative
        if text.len() <= 80 && !text.ends_with('.') && is_likely_heading_text(text) && ratio >= 0.99
        {
            // Skip for now to avoid false positives
        }

        BlockKind::Paragraph
    }

    fn font_size_to_heading_level(&self, font_size: f32) -> u8 {
        let ratio = font_size / self.config.body_font_size;
        if ratio >= 2.0 {
            1
        } else if ratio >= 1.6 {
            2
        } else if ratio >= 1.35 {
            3
        } else if ratio >= 1.15 {
            4
        } else {
            5
        }
    }

    fn is_in_header_zone(&self, block: &RawTextBlock, page: &RawPage) -> bool {
        block.bbox.y1 <= page.height * self.config.header_footer_zone
    }

    fn is_in_footer_zone(&self, block: &RawTextBlock, page: &RawPage) -> bool {
        block.bbox.y0 >= page.height * (1.0 - self.config.header_footer_zone)
    }
}

/// Compute the body font size as the statistical mode across all blocks in the document.
/// Uses 0.5pt histogram bins.
fn compute_body_font_size(raw_pages: &[RawPage]) -> f32 {
    use std::collections::HashMap;

    let mut histogram: HashMap<u32, usize> = HashMap::new();

    for page in raw_pages {
        for block in &page.blocks {
            if block.font_size > 0.0 {
                // Bin to nearest 0.5pt: multiply by 2, round, store as integer key
                let key = (block.font_size * 2.0).round() as u32;
                *histogram.entry(key).or_insert(0) += 1;
            }
        }
    }

    if histogram.is_empty() {
        return 10.0; // fallback
    }

    let mode_key = histogram
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(key, _)| key)
        .unwrap_or(20); // 20 → 10.0pt

    mode_key as f32 / 2.0
}

/// Estimate indent depth from x position (0 = leftmost, higher = more indented).
fn indent_depth(block: &RawTextBlock, page: &RawPage) -> u8 {
    let x_fraction = block.bbox.x0 / page.width;
    if x_fraction < 0.15 {
        0
    } else if x_fraction < 0.25 {
        1
    } else {
        2
    }
}

/// Heuristic: is this text likely a heading by its content alone?
/// Checks for all-caps or title-case (most words capitalised, short).
fn is_likely_heading_text(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || words.len() > 12 {
        return false;
    }
    let cap_count = words
        .iter()
        .filter(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
        .count();
    // Title case: >= 60% of words capitalised
    cap_count as f32 / words.len() as f32 >= 0.6
}

/// Pre-classify blocks to identify which ones should be excluded from table detection.
/// Returns true for blocks that are clearly not table cells (headings, captions, list items, etc.).
fn is_non_table_block(block: &RawTextBlock, body_font_size: f32) -> bool {
    let text = block.text.trim();

    // Empty blocks
    if text.is_empty() {
        return true;
    }

    // Captions (e.g. "Table E1.1 ...", "Figure 3. ...")
    if caption_re().is_match(text) {
        return true;
    }

    // Page numbers
    if page_number_re().is_match(text) && block.bbox.width() < 40.0 && text.len() <= 6 {
        return true;
    }

    // List items
    if ordered_list_re().is_match(text) || unordered_list_re().is_match(text) {
        return true;
    }

    // Author / affiliation blocks on scholarly first pages are often arranged
    // in a visual grid, but rendering them as markdown tables reads poorly.
    if looks_like_author_affiliation_block(text) {
        return true;
    }

    // Headings: font size significantly larger than body
    if body_font_size > 0.0 && block.font_size / body_font_size >= 1.15 {
        return true;
    }

    // Long paragraph text (table cells are typically short)
    // A block with > 200 chars and no column-like structure is likely a paragraph
    if text.len() > 200 {
        return true;
    }

    false
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

/// Detect table cells by finding blocks arranged in a 2D grid.
/// Uses a region-based approach: identifies candidate table regions among
/// non-heading/non-caption blocks, then validates each region independently.
/// Returns a map from block_id → BlockKind::TableCell { row, col }.
#[cfg(test)]
fn detect_table_cells(blocks: &[RawTextBlock]) -> std::collections::HashMap<usize, BlockKind> {
    // Use body_font_size=0 when we can't compute it (disables heading filter in is_non_table_block)
    detect_table_cells_with_font_size(blocks, 0.0)
}

fn detect_table_cells_with_font_size(
    blocks: &[RawTextBlock],
    body_font_size: f32,
) -> std::collections::HashMap<usize, BlockKind> {
    use std::collections::HashMap;

    let mut result = HashMap::new();

    if blocks.len() < 4 {
        return result; // need at least a 2x2 grid
    }

    // Filter out blocks that are clearly not table cells
    let candidate_blocks: Vec<&RawTextBlock> = blocks
        .iter()
        .filter(|b| !is_non_table_block(b, body_font_size))
        .collect();

    if candidate_blocks.len() < 4 {
        return result;
    }

    // Find candidate table regions: groups of vertically contiguous blocks
    // that share column alignment.
    let regions = find_table_regions(&candidate_blocks);

    for region in &regions {
        if region.len() < 4 {
            continue;
        }

        let region_result = detect_table_in_region(region);
        // Merge into overall result
        for (block_id, kind) in region_result {
            result.insert(block_id, kind);
        }
    }

    result
}

/// Find contiguous vertical regions of blocks that could be tables.
/// Groups blocks by vertical proximity — blocks within a region have small
/// vertical gaps between consecutive rows.
fn find_table_regions<'a>(blocks: &[&'a RawTextBlock]) -> Vec<Vec<&'a RawTextBlock>> {
    if blocks.is_empty() {
        return vec![];
    }

    // Sort by y0 (top edge)
    let mut sorted: Vec<&RawTextBlock> = blocks.to_vec();
    sorted.sort_by(|a, b| {
        a.bbox
            .y0
            .partial_cmp(&b.bbox.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Cluster y-positions into rows
    let y_positions: Vec<f32> = sorted.iter().map(|b| b.bbox.y0).collect();
    let y_clusters = cluster_positions(&y_positions, 6.0);

    if y_clusters.len() < 2 {
        return vec![];
    }

    // Assign each block to its row cluster
    let mut row_assignments: Vec<(usize, &RawTextBlock)> = Vec::new();
    for &block in &sorted {
        if let Some(row) = nearest_cluster(block.bbox.y0, &y_clusters, 6.0) {
            row_assignments.push((row, block));
        }
    }

    // Group by row
    let mut rows_map: std::collections::BTreeMap<usize, Vec<&RawTextBlock>> =
        std::collections::BTreeMap::new();
    for (row, block) in &row_assignments {
        rows_map.entry(*row).or_default().push(*block);
    }

    // A row needs >= 2 blocks to be table-like
    let table_rows: Vec<usize> = rows_map
        .iter()
        .filter(|(_, blocks_in_row)| blocks_in_row.len() >= 2)
        .map(|(row, _)| *row)
        .collect();

    if table_rows.len() < 2 {
        return vec![];
    }

    // Find contiguous runs of table-like rows
    let mut regions: Vec<Vec<&'a RawTextBlock>> = Vec::new();
    let mut current_region: Vec<&RawTextBlock> = Vec::new();
    let mut prev_row: Option<usize> = None;

    for &row_idx in &table_rows {
        let is_contiguous = match prev_row {
            Some(prev) => row_idx <= prev + 2, // allow one gap row
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

/// Detect a table within a candidate region of blocks.
/// Returns a map from block_id → BlockKind::TableCell { row, col }.
fn detect_table_in_region(region: &[&RawTextBlock]) -> std::collections::HashMap<usize, BlockKind> {
    use std::collections::HashMap;

    let mut result = HashMap::new();

    // Numeric columns are often right-aligned, so use the right edge for
    // numeric-looking cells and the left edge everywhere else.
    let x_positions: Vec<f32> = region.iter().map(|b| table_column_anchor(b)).collect();
    let x_clusters = cluster_positions(&x_positions, 12.0);

    // Cluster y-positions (top edges) into rows
    let y_positions: Vec<f32> = region.iter().map(|b| b.bbox.y0).collect();
    let y_clusters = cluster_positions(&y_positions, 6.0);

    // Need >= 2 rows and >= 2 columns
    if x_clusters.len() < 2 || y_clusters.len() < 2 {
        return result;
    }

    // Guard: a real table rarely has more than 10 columns
    if x_clusters.len() > 10 {
        return result;
    }

    // Assign each block to a (row, col) if it aligns to cluster centres,
    // keyed by block_id for stability against reordering.
    for block in region.iter() {
        let col = nearest_cluster(table_column_anchor(block), &x_clusters, 12.0);
        let row = nearest_cluster(block.bbox.y0, &y_clusters, 6.0);
        if let (Some(col), Some(row)) = (col, row) {
            result.insert(block.block_id, BlockKind::TableCell { row, col });
        }
    }

    // Only keep as table if at least 4 cells were assigned (2x2 minimum)
    if result.len() < 4 {
        result.clear();
        return result;
    }

    // Validate: need at least 2 columns each with >= 2 cells
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

    // Guard: table height can be up to 85% of estimated page height
    // (engineering standards often have full-page tables)
    let assigned_blocks: Vec<&&RawTextBlock> = region
        .iter()
        .filter(|b| result.contains_key(&b.block_id))
        .collect();

    if let (Some(min_y), Some(max_y)) = (
        assigned_blocks.iter().map(|b| b.bbox.y0).reduce(f32::min),
        assigned_blocks.iter().map(|b| b.bbox.y1).reduce(f32::max),
    ) {
        let table_height = max_y - min_y;
        // Only reject if table height is unreasonably large compared to itself
        // (this shouldn't happen for a real region, but guards against pathological cases)
        if table_height < 0.0 {
            result.clear();
        }
    }

    result
}

/// Cluster a list of float positions using a simple greedy merge.
/// Returns the list of cluster centre values, sorted ascending.
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

/// Find which cluster index a value belongs to, within tolerance. Returns None if no match.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::types::Bbox;

    fn make_page(width: f32, height: f32, blocks: Vec<RawTextBlock>) -> RawPage {
        RawPage {
            page_num: 0,
            width,
            height,
            blocks,
            image_refs: vec![],
        }
    }

    fn make_block(x0: f32, y0: f32, x1: f32, y1: f32, text: &str, font_size: f32) -> RawTextBlock {
        make_block_id(x0, y0, x1, y1, text, font_size, 0)
    }

    fn make_block_id(
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        text: &str,
        font_size: f32,
        block_id: usize,
    ) -> RawTextBlock {
        RawTextBlock {
            bbox: Bbox::new(x0, y0, x1, y1),
            text: text.to_string(),
            font_size,
            font_name: "unknown".to_string(),
            page_num: 0,
            block_id,
            reading_order: 0,
        }
    }

    #[test]
    fn heading_detected_by_font_size() {
        let page = make_page(600.0, 800.0, vec![]);
        let config = ClassifierConfig {
            body_font_size: 10.0,
            heading_size_ratio: 1.15,
            header_footer_zone: 0.07,
        };
        let clf = Classifier::with_config(config);
        let block = make_block(50.0, 100.0, 400.0, 130.0, "Introduction", 18.0);
        let kind = clf.classify_block(&block, &page);
        assert!(matches!(kind, BlockKind::Heading { level: 2 }));
    }

    #[test]
    fn paragraph_at_body_size() {
        let page = make_page(600.0, 800.0, vec![]);
        let clf = Classifier::with_config(ClassifierConfig::default());
        let block = make_block(
            50.0,
            200.0,
            550.0,
            215.0,
            "This is a normal paragraph.",
            10.0,
        );
        assert_eq!(clf.classify_block(&block, &page), BlockKind::Paragraph);
    }

    #[test]
    fn page_number_standalone_digit() {
        let page = make_page(600.0, 800.0, vec![]);
        let clf = Classifier::with_config(ClassifierConfig::default());
        let block = make_block(280.0, 400.0, 320.0, 415.0, "42", 10.0);
        assert_eq!(clf.classify_block(&block, &page), BlockKind::PageNumber);
    }

    #[test]
    fn running_header_in_top_zone() {
        let page = make_page(600.0, 800.0, vec![]);
        let clf = Classifier::with_config(ClassifierConfig::default());
        // y1 = 30 <= 800 * 0.07 = 56
        let block = make_block(50.0, 10.0, 400.0, 30.0, "Chapter 1: Overview", 9.0);
        assert_eq!(clf.classify_block(&block, &page), BlockKind::RunningHeader);
    }

    #[test]
    fn ordered_list_item() {
        let page = make_page(600.0, 800.0, vec![]);
        let clf = Classifier::with_config(ClassifierConfig::default());
        let block = make_block(50.0, 200.0, 500.0, 215.0, "1. First item", 10.0);
        assert!(matches!(
            clf.classify_block(&block, &page),
            BlockKind::ListItem { ordered: true, .. }
        ));
    }

    #[test]
    fn unordered_list_item() {
        let page = make_page(600.0, 800.0, vec![]);
        let clf = Classifier::with_config(ClassifierConfig::default());
        let block = make_block(50.0, 200.0, 500.0, 215.0, "• Bullet point", 10.0);
        assert!(matches!(
            clf.classify_block(&block, &page),
            BlockKind::ListItem { ordered: false, .. }
        ));
    }

    #[test]
    fn caption_detected() {
        let page = make_page(600.0, 800.0, vec![]);
        let clf = Classifier::with_config(ClassifierConfig::default());
        let block = make_block(50.0, 400.0, 500.0, 415.0, "Figure 1. A diagram.", 9.0);
        assert_eq!(clf.classify_block(&block, &page), BlockKind::Caption);
    }

    #[test]
    fn body_font_size_computed_as_mode() {
        let block = |fs: f32| RawTextBlock {
            bbox: Bbox::new(0.0, 0.0, 100.0, 20.0),
            text: "x".to_string(),
            font_size: fs,
            font_name: "unknown".to_string(),
            page_num: 0,
            block_id: 0,
            reading_order: 0,
        };
        let pages = vec![RawPage {
            page_num: 0,
            width: 600.0,
            height: 800.0,
            blocks: vec![
                block(12.0),
                block(12.0),
                block(12.0),
                block(18.0),
                block(24.0),
            ],
            image_refs: vec![],
        }];
        assert_eq!(compute_body_font_size(&pages), 12.0);
    }

    #[test]
    fn table_cell_detection_2x2() {
        let blocks = vec![
            make_block_id(50.0, 100.0, 150.0, 120.0, "A1", 10.0, 0),
            make_block_id(200.0, 100.0, 300.0, 120.0, "A2", 10.0, 1),
            make_block_id(50.0, 130.0, 150.0, 150.0, "B1", 10.0, 2),
            make_block_id(200.0, 130.0, 300.0, 150.0, "B2", 10.0, 3),
        ];
        let cells = detect_table_cells(&blocks);
        assert_eq!(cells.len(), 4);
        assert!(cells
            .values()
            .all(|k| matches!(k, BlockKind::TableCell { .. })));
    }

    #[test]
    fn table_detection_realistic_engineering_page() {
        // Simulates an engineering standards page:
        // - Title block at top (large font, heading)
        // - 3x4 table data grid (12 cells, ~60% of blocks)
        // - Notes block below
        // Total: 14 blocks, table is the majority
        let mut blocks = Vec::new();
        let mut id = 0;

        // Title: "TABLE E1.1 Selection Table" — large font (heading)
        blocks.push(make_block_id(
            50.0,
            30.0,
            500.0,
            55.0,
            "TABLE E1.1 Selection Table for Application",
            14.0,
            id,
        ));
        id += 1;

        // Table header row (3 columns)
        blocks.push(make_block_id(
            50.0,
            80.0,
            180.0,
            100.0,
            "Cross Section",
            10.0,
            id,
        ));
        id += 1;
        blocks.push(make_block_id(
            200.0,
            80.0,
            350.0,
            100.0,
            "Limit State",
            10.0,
            id,
        ));
        id += 1;
        blocks.push(make_block_id(
            370.0,
            80.0,
            520.0,
            100.0,
            "Reference",
            10.0,
            id,
        ));
        id += 1;

        // Data row 1
        blocks.push(make_block_id(
            50.0, 110.0, 180.0, 130.0, "W-shape", 10.0, id,
        ));
        id += 1;
        blocks.push(make_block_id(
            200.0,
            110.0,
            350.0,
            130.0,
            "Flexural Yielding",
            10.0,
            id,
        ));
        id += 1;
        blocks.push(make_block_id(
            370.0,
            110.0,
            520.0,
            130.0,
            "Section F2",
            10.0,
            id,
        ));
        id += 1;

        // Data row 2
        blocks.push(make_block_id(
            50.0, 140.0, 180.0, 160.0, "Channel", 10.0, id,
        ));
        id += 1;
        blocks.push(make_block_id(200.0, 140.0, 350.0, 160.0, "LTB", 10.0, id));
        id += 1;
        blocks.push(make_block_id(
            370.0,
            140.0,
            520.0,
            160.0,
            "Section F3",
            10.0,
            id,
        ));
        id += 1;

        // Data row 3
        blocks.push(make_block_id(50.0, 170.0, 180.0, 190.0, "HSS", 10.0, id));
        id += 1;
        blocks.push(make_block_id(
            200.0,
            170.0,
            350.0,
            190.0,
            "Local Buckling",
            10.0,
            id,
        ));
        id += 1;
        blocks.push(make_block_id(
            370.0,
            170.0,
            520.0,
            190.0,
            "Section F7",
            10.0,
            id,
        ));
        id += 1;

        // Notes below table
        blocks.push(make_block_id(
            50.0,
            210.0,
            520.0,
            240.0,
            "User Note: See Commentary for further discussion.",
            10.0,
            id,
        ));

        // Use body_font_size=10.0 so the 14pt title is detected as heading and excluded
        let cells = detect_table_cells_with_font_size(&blocks, 10.0);

        // Should detect 12 table cells (3 cols x 4 rows)
        assert!(
            cells.len() >= 12,
            "Expected >= 12 table cells, got {} (cells: {:?})",
            cells.len(),
            cells
        );
        assert!(cells
            .values()
            .all(|k| matches!(k, BlockKind::TableCell { .. })));

        // Title (id=0) and notes (id=13) should NOT be in the table
        assert!(!cells.contains_key(&0), "Title should not be a table cell");
        assert!(!cells.contains_key(&13), "Notes should not be a table cell");
    }

    #[test]
    fn table_detection_full_page_table_not_rejected() {
        // A full-page table (occupying ~70% of page height) should still be detected.
        // Previously Guard 2 (40% height limit) would reject this.
        let mut blocks = Vec::new();
        let mut id = 0;

        // 5 columns x 8 rows = 40 cells spanning y=100 to y=660 on an 800pt page
        for row in 0..8 {
            for col in 0..5 {
                let x0 = 50.0 + col as f32 * 100.0;
                let y0 = 100.0 + row as f32 * 80.0;
                blocks.push(make_block_id(
                    x0,
                    y0,
                    x0 + 80.0,
                    y0 + 20.0,
                    &format!("R{}C{}", row, col),
                    10.0,
                    id,
                ));
                id += 1;
            }
        }

        let cells = detect_table_cells(&blocks);
        assert!(
            cells.len() >= 40,
            "Full-page table should be detected, got {} cells",
            cells.len()
        );
    }

    #[test]
    fn table_detection_excludes_headings_and_captions() {
        // Blocks with heading font sizes or caption patterns should be excluded
        let blocks = vec![
            // Heading (large font)
            make_block_id(50.0, 30.0, 400.0, 55.0, "CHAPTER 5", 18.0, 0),
            // Caption
            make_block_id(
                50.0,
                60.0,
                400.0,
                75.0,
                "Table 5.1: Material Properties",
                10.0,
                1,
            ),
            // 2x2 table
            make_block_id(50.0, 100.0, 180.0, 120.0, "Steel", 10.0, 2),
            make_block_id(200.0, 100.0, 350.0, 120.0, "Fy = 50 ksi", 10.0, 3),
            make_block_id(50.0, 130.0, 180.0, 150.0, "Aluminum", 10.0, 4),
            make_block_id(200.0, 130.0, 350.0, 150.0, "Fy = 35 ksi", 10.0, 5),
        ];

        let cells = detect_table_cells_with_font_size(&blocks, 10.0);

        // Table cells should be detected (ids 2-5)
        assert_eq!(cells.len(), 4, "Should detect 4 table cells");
        // Heading and caption should not be table cells
        assert!(
            !cells.contains_key(&0),
            "Heading should not be a table cell"
        );
        assert!(
            !cells.contains_key(&1),
            "Caption should not be a table cell"
        );
    }

    #[test]
    fn table_detection_handles_right_aligned_numeric_columns() {
        let blocks = vec![
            make_block_id(40.0, 80.0, 220.0, 100.0, "Line item", 10.0, 0),
            make_block_id(290.0, 80.0, 360.0, 100.0, "2024", 10.0, 1),
            make_block_id(380.0, 80.0, 450.0, 100.0, "2023", 10.0, 2),
            make_block_id(40.0, 112.0, 220.0, 132.0, "Revenue", 10.0, 3),
            make_block_id(315.0, 112.0, 360.0, 132.0, "120.062.000", 10.0, 4),
            make_block_id(405.0, 112.0, 450.0, 132.0, "124.406.000", 10.0, 5),
            make_block_id(40.0, 144.0, 220.0, 164.0, "EBITDA", 10.0, 6),
            make_block_id(322.0, 144.0, 360.0, 164.0, "8.509.000", 10.0, 7),
            make_block_id(412.0, 144.0, 450.0, 164.0, "10.808.000", 10.0, 8),
        ];

        let cells = detect_table_cells_with_font_size(&blocks, 10.0);
        assert_eq!(cells.len(), 9, "all right-aligned cells should be kept");

        let row0_cols: Vec<usize> = (0..=2)
            .map(|id| match cells.get(&id) {
                Some(BlockKind::TableCell { col, .. }) => *col,
                other => panic!("expected table cell for id {id}, got {other:?}"),
            })
            .collect();
        assert_eq!(row0_cols, vec![0, 1, 2]);
    }

    // ========================================================================
    // Metadata-aware classification (Phase 3)
    // ========================================================================

    use crate::pdf::metadata::{FontInfo, PageMetadata, StructTag};

    fn classifier_with_body(size: f32) -> Classifier {
        Classifier::with_config(ClassifierConfig {
            body_font_size: size,
            ..Default::default()
        })
    }

    fn page_for_classifier_tests() -> RawPage {
        RawPage {
            page_num: 0,
            width: 612.0,
            height: 792.0,
            blocks: Vec::new(),
            image_refs: Vec::new(),
        }
    }

    #[test]
    fn metadata_none_is_identical_to_classify_block() {
        let clf = classifier_with_body(10.0);
        let page = page_for_classifier_tests();
        let block = make_block(100.0, 100.0, 500.0, 115.0, "A section header at 11pt", 11.0);

        let without = clf.classify_block(&block, &page);
        let with_none = clf.classify_block_with_metadata(&block, &page, None);
        assert_eq!(without, with_none);
    }

    #[test]
    fn scholarly_metadata_does_not_become_heading() {
        let clf = classifier_with_body(10.0);
        let page = page_for_classifier_tests();
        let block = make_block(
            100.0,
            100.0,
            500.0,
            120.0,
            "arXiv:1706.03762v7  [cs.CL]  2 Aug 2023",
            16.0,
        );

        assert_eq!(
            clf.classify_block(&block, &page),
            BlockKind::Paragraph,
            "scholarly metadata should stay paragraph even at large font size"
        );
    }

    #[test]
    fn author_cells_with_emails_are_not_table_cells() {
        let blocks = vec![
            make_block_id(
                50.0,
                80.0,
                220.0,
                140.0,
                "Ashish Vaswani\nGoogle Brain\navaswani@google.com",
                10.0,
                0,
            ),
            make_block_id(
                260.0,
                80.0,
                430.0,
                140.0,
                "Noam Shazeer\nGoogle Brain\nnoam@google.com",
                10.0,
                1,
            ),
            make_block_id(50.0, 160.0, 430.0, 180.0, "Abstract", 12.0, 2),
            make_block_id(
                50.0,
                200.0,
                430.0,
                220.0,
                "The Transformer replaces recurrence with attention.",
                10.0,
                3,
            ),
        ];

        let cells = detect_table_cells_with_font_size(&blocks, 10.0);
        assert!(
            cells.is_empty(),
            "author contact blocks should not be classified as markdown tables"
        );
    }

    #[test]
    fn bold_at_body_size_becomes_heading_with_metadata() {
        let clf = classifier_with_body(10.0);
        let page = page_for_classifier_tests();
        // Body-size (10pt) short line — ordinarily a Paragraph.
        let block = make_block(100.0, 100.0, 500.0, 112.0, "Methods", 10.0);

        let without_md = clf.classify_block_with_metadata(&block, &page, None);
        assert_eq!(
            without_md,
            BlockKind::Paragraph,
            "no metadata → stays paragraph"
        );

        // Now with metadata saying that bbox is bold (weight 700).
        let mut md = PageMetadata::default();
        md.fonts.push((
            block.bbox,
            FontInfo {
                family: "Helvetica-Bold".to_string(),
                weight: 700,
                italic: false,
            },
        ));

        let with_md = clf.classify_block_with_metadata(&block, &page, Some(&md));
        assert_eq!(with_md, BlockKind::Heading { level: 4 });
    }

    #[test]
    fn bold_long_sentence_is_not_heading() {
        let clf = classifier_with_body(10.0);
        let page = page_for_classifier_tests();
        // Bold long paragraph-ish — ends with period. Don't upgrade.
        let block = make_block(
            100.0,
            100.0,
            500.0,
            115.0,
            "This is a perfectly ordinary paragraph written in bold because the author enjoys emphasis, and it ends with a sentence-terminating period.",
            10.0,
        );
        let mut md = PageMetadata::default();
        md.fonts.push((
            block.bbox,
            FontInfo {
                family: "Times-Bold".to_string(),
                weight: 700,
                italic: false,
            },
        ));
        assert_eq!(
            clf.classify_block_with_metadata(&block, &page, Some(&md)),
            BlockKind::Paragraph,
            "bold running prose is not a heading"
        );
    }

    #[test]
    fn struct_tree_role_h2_wins_over_size_ratio() {
        let clf = classifier_with_body(10.0);
        let page = page_for_classifier_tests();
        // Same-size-as-body block — not a heading by size.
        let block = make_block(100.0, 100.0, 500.0, 115.0, "Background", 10.0);

        let mut md = PageMetadata::default();
        md.struct_tags.push(StructTag {
            bbox: block.bbox,
            role: "H2".to_string(),
            alt: None,
        });

        assert_eq!(
            clf.classify_block_with_metadata(&block, &page, Some(&md)),
            BlockKind::Heading { level: 2 }
        );
    }

    #[test]
    fn struct_tree_title_maps_to_h1() {
        let clf = classifier_with_body(10.0);
        let page = page_for_classifier_tests();
        let block = make_block(100.0, 100.0, 500.0, 115.0, "My Thesis", 10.0);

        let mut md = PageMetadata::default();
        md.struct_tags.push(StructTag {
            bbox: block.bbox,
            role: "Title".to_string(),
            alt: None,
        });

        assert_eq!(
            clf.classify_block_with_metadata(&block, &page, Some(&md)),
            BlockKind::Heading { level: 1 }
        );
    }

    #[test]
    fn unknown_struct_role_falls_through_to_size_detection() {
        let clf = classifier_with_body(10.0);
        let page = page_for_classifier_tests();
        let block = make_block(100.0, 100.0, 500.0, 115.0, "Some text", 10.0);

        let mut md = PageMetadata::default();
        md.struct_tags.push(StructTag {
            bbox: block.bbox,
            role: "NonsenseRole".to_string(),
            alt: None,
        });

        // Should fall through — size is body, so Paragraph.
        assert_eq!(
            clf.classify_block_with_metadata(&block, &page, Some(&md)),
            BlockKind::Paragraph
        );
    }
}
