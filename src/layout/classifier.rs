use regex::Regex;
use std::sync::OnceLock;
use crate::document::types::{Block, BlockKind, RawPage, RawTextBlock};

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
        Regex::new(r"(?i)^\s*(figure|fig\.?|table|tbl\.?|algorithm|listing|exhibit)\s+[\dIVXivx]+[.:)]").unwrap()
    })
}

fn code_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Heuristic: starts with common code patterns
    RE.get_or_init(|| Regex::new(r"^\s*(```|~~~|def |fn |pub |class |import |from |#include|int |void |return )").unwrap())
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
    pub fn classify_page(&self, raw_blocks: Vec<RawTextBlock>, page: &RawPage) -> Vec<Block> {
        // First pass: detect table cells
        let table_cells = detect_table_cells(&raw_blocks);

        raw_blocks
            .into_iter()
            .enumerate()
            .map(|(i, rb)| {
                let kind = if let Some(tc) = table_cells.get(&i) {
                    tc.clone()
                } else {
                    self.classify_block(&rb, page)
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

    fn classify_block(&self, block: &RawTextBlock, page: &RawPage) -> BlockKind {
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
            return BlockKind::ListItem { ordered: true, depth };
        }
        if unordered_list_re().is_match(text) {
            let depth = indent_depth(block, page);
            return BlockKind::ListItem { ordered: false, depth };
        }

        // Heading detection — font size based
        let ratio = block.font_size / self.config.body_font_size;
        if ratio >= self.config.heading_size_ratio {
            let level = self.font_size_to_heading_level(block.font_size);
            return BlockKind::Heading { level };
        }

        // Short, single-line text at larger-ish size (section headers with same body size)
        // Heuristic: <= 80 chars, no trailing period, all-caps or title-case dominant
        // Only apply if we couldn't detect via size — weak signal, be conservative
        if text.len() <= 80 && !text.ends_with('.') && is_likely_heading_text(text) && ratio >= 0.99 {
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

/// Detect table cells by finding blocks arranged in a 2D grid.
/// Returns a map from block index → BlockKind::TableCell { row, col }.
fn detect_table_cells(blocks: &[RawTextBlock]) -> std::collections::HashMap<usize, BlockKind> {
    use std::collections::HashMap;

    let mut result = HashMap::new();

    if blocks.len() < 4 {
        return result; // need at least a 2x2 grid
    }

    // Cluster x-positions (left edges) into columns
    let x_positions: Vec<f32> = blocks.iter().map(|b| b.bbox.x0).collect();
    let x_clusters = cluster_positions(&x_positions, 8.0);

    // Cluster y-positions (top edges) into rows
    let y_positions: Vec<f32> = blocks.iter().map(|b| b.bbox.y0).collect();
    let y_clusters = cluster_positions(&y_positions, 6.0);

    // Only treat as table if we have >= 2 rows and >= 2 columns
    if x_clusters.len() < 2 || y_clusters.len() < 2 {
        return result;
    }

    // Assign each block to a (row, col) if it aligns to cluster centres
    for (i, block) in blocks.iter().enumerate() {
        let col = nearest_cluster(block.bbox.x0, &x_clusters, 8.0);
        let row = nearest_cluster(block.bbox.y0, &y_clusters, 6.0);
        if let (Some(col), Some(row)) = (col, row) {
            result.insert(i, BlockKind::TableCell { row, col });
        }
    }

    // Only keep as table if at least 4 cells were assigned (2x2 minimum)
    if result.len() < 4 {
        result.clear();
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
        RawTextBlock {
            bbox: Bbox::new(x0, y0, x1, y1),
            text: text.to_string(),
            font_size,
            font_name: "unknown".to_string(),
            page_num: 0,
            block_id: 0,
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
        let block = make_block(50.0, 200.0, 550.0, 215.0, "This is a normal paragraph.", 10.0);
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
            blocks: vec![block(12.0), block(12.0), block(12.0), block(18.0), block(24.0)],
            image_refs: vec![],
        }];
        assert_eq!(compute_body_font_size(&pages), 12.0);
    }

    #[test]
    fn table_cell_detection_2x2() {
        let blocks = vec![
            make_block(50.0, 100.0, 150.0, 120.0, "A1", 10.0),
            make_block(200.0, 100.0, 300.0, 120.0, "A2", 10.0),
            make_block(50.0, 130.0, 150.0, 150.0, "B1", 10.0),
            make_block(200.0, 130.0, 300.0, 150.0, "B2", 10.0),
        ];
        let cells = detect_table_cells(&blocks);
        assert_eq!(cells.len(), 4);
        assert!(cells.values().all(|k| matches!(k, BlockKind::TableCell { .. })));
    }
}
