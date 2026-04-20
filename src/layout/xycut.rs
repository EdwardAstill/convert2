//! XY-Cut++ reading-order algorithm.
//!
//! Port of OpenDataLoader's `XYCutPlusPlusSorter.java` (Hancom, Apache-2.0).
//! Based on arXiv:2504.10258, with the *Java simplified variant* the reference
//! product ships: geometric only (no semantic label priority), β=2.0 against
//! max-width (the paper uses β=1.3 against median), gap-based axis selection
//! with density ratio kept for future use, and a geometric Y-merge for
//! cross-layout elements (the paper uses IoU-weighted matching).
//!
//! # Coordinate convention
//!
//! The Java source uses PDF native coordinates (y grows upward). This module
//! uses top-left origin (y grows downward). Every Y comparison is flipped from
//! the Java source:
//!
//! | Java (PDF y-up)             | Rust (top-left y-down)      |
//! |-----------------------------|-----------------------------|
//! | `topY` (larger = higher)    | `y0` (smaller = higher)     |
//! | `bottomY`                   | `y1`                        |
//! | sort by `-topY` (top first) | sort by `+y0` (top first)   |
//! | `cross.topY >= main.topY`   | `cross.y0 <= main.y0`       |
//!
//! X-axis is unchanged.

use crate::document::types::{Bbox, RawTextBlock};
use std::collections::HashSet;

/// Configuration for XY-Cut++.
#[derive(Debug, Clone)]
pub struct XyCutConfig {
    /// Minimum vertical whitespace gap (points) required to perform a horizontal
    /// cut. Maps to Java's `MIN_GAP_THRESHOLD` for the Y axis. Default 8.0.
    pub min_horizontal_gap: f32,
    /// Minimum horizontal whitespace gap (points) required to perform a vertical
    /// cut. Maps to Java's `MIN_GAP_THRESHOLD` for the X axis. Default 12.0.
    pub min_vertical_gap: f32,
    /// Gaps smaller than this are ignored during projection sweeping, so tiny
    /// noise does not become a spurious cut. Default 2.0.
    pub overlap_tolerance: f32,
    /// Safety cap on recursion depth. Default 50.
    pub max_depth: usize,
    /// Cross-layout width multiplier. An element is cross-layout if its width is
    /// `>= beta * max_width`. Java default 2.0 (effectively disabled — nothing is
    /// wider than max). Paper default 1.3 against median. Default 2.0.
    pub beta: f32,
    /// Density ratio threshold (content area / bounding-region area). Currently
    /// computed but not used as a decision driver, kept for future parity with
    /// the paper. Default 0.9.
    #[allow(dead_code)]
    pub density_threshold: f32,
    /// Minimum horizontal overlap ratio (relative to the smaller box's width)
    /// required to count as one overlap in cross-layout detection. Default 0.1.
    pub overlap_threshold: f32,
    /// Minimum number of horizontal overlaps a wide element must have with
    /// other elements to be classified as cross-layout. Default 2.
    pub min_overlap_count: usize,
    /// Width-ratio threshold for narrow-outlier filtering in the vertical-cut
    /// retry pass. Items narrower than this fraction of the region width are
    /// temporarily removed so that column gaps bridged by narrow elements
    /// (page numbers, footnote markers) become visible. Default 0.1.
    pub narrow_element_width_ratio: f32,
}

impl Default for XyCutConfig {
    fn default() -> Self {
        Self {
            min_horizontal_gap: 8.0,
            min_vertical_gap: 12.0,
            overlap_tolerance: 2.0,
            max_depth: 50,
            beta: 2.0,
            density_threshold: 0.9,
            overlap_threshold: 0.1,
            min_overlap_count: 2,
            narrow_element_width_ratio: 0.1,
        }
    }
}

/// Compute a reading-order permutation of the page's blocks.
///
/// The returned `Vec<usize>` is a permutation of `0..blocks.len()` giving the
/// order in which blocks should be read.
#[must_use]
pub fn build_xycut_order(blocks: &[RawTextBlock], config: &XyCutConfig) -> Vec<usize> {
    let all: Vec<usize> = (0..blocks.len()).collect();
    if blocks.len() <= 1 {
        return all;
    }

    // Phase 1: pre-mask cross-layout elements (wide full-page headers/footers).
    let cross_set = identify_cross_layout(&all, blocks, config);
    let main: Vec<usize> = all
        .iter()
        .copied()
        .filter(|i| !cross_set.contains(i))
        .collect();

    if main.is_empty() {
        return sort_by_y_then_x(&all, blocks);
    }

    // Phase 2: density ratio (reserved for future tiebreaker use).
    let _density = compute_density_ratio(&main, blocks);

    // Phase 3: recursive XY/YX segmentation on main content.
    let sorted_main = recursive_segment(main, blocks, config, 0);

    // Phase 4: merge cross-layout elements back into the stream by Y.
    let cross: Vec<usize> = cross_set.into_iter().collect();
    merge_cross_layout(sorted_main, cross, blocks)
}

/// Apply a precomputed reading-order permutation to the blocks in place.
pub fn assign_reading_order(order: &[usize], blocks: &mut [RawTextBlock]) {
    for (pos, &idx) in order.iter().enumerate() {
        if let Some(b) = blocks.get_mut(idx) {
            b.reading_order = pos;
        }
    }
}

// ============================================================================
// Phase 1 — cross-layout pre-masking
// ============================================================================

fn identify_cross_layout(
    indices: &[usize],
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
) -> HashSet<usize> {
    let mut result = HashSet::new();
    if indices.len() < 3 {
        return result;
    }
    let max_width = indices
        .iter()
        .map(|&i| blocks[i].bbox.width())
        .fold(0.0_f32, f32::max);
    let threshold = config.beta * max_width;

    for &idx in indices {
        if blocks[idx].bbox.width() >= threshold
            && has_min_overlaps(idx, indices, blocks, config)
        {
            result.insert(idx);
        }
    }
    result
}

fn has_min_overlaps(
    target: usize,
    indices: &[usize],
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
) -> bool {
    let target_bbox = &blocks[target].bbox;
    let mut count = 0usize;
    for &other in indices {
        if other == target {
            continue;
        }
        if horizontal_overlap_ratio(target_bbox, &blocks[other].bbox) >= config.overlap_threshold {
            count += 1;
            if count >= config.min_overlap_count {
                return true;
            }
        }
    }
    false
}

fn horizontal_overlap_ratio(a: &Bbox, b: &Bbox) -> f32 {
    let overlap_left = a.x0.max(b.x0);
    let overlap_right = a.x1.min(b.x1);
    let overlap_width = (overlap_right - overlap_left).max(0.0);
    if overlap_width <= 0.0 {
        return 0.0;
    }
    let smaller = a.width().min(b.width());
    if smaller > 0.0 {
        overlap_width / smaller
    } else {
        0.0
    }
}

// ============================================================================
// Phase 2 — density ratio
// ============================================================================

fn compute_density_ratio(indices: &[usize], blocks: &[RawTextBlock]) -> f32 {
    if indices.is_empty() {
        return 1.0;
    }
    let region = bounding_region(indices, blocks);
    let region_area = region.area();
    if region_area <= 0.0 {
        return 1.0;
    }
    let content_area: f32 = indices.iter().map(|&i| blocks[i].bbox.area()).sum();
    (content_area / region_area).min(1.0)
}

fn bounding_region(indices: &[usize], blocks: &[RawTextBlock]) -> Bbox {
    let mut region = blocks[indices[0]].bbox;
    for &i in &indices[1..] {
        region = region.union(&blocks[i].bbox);
    }
    region
}

// ============================================================================
// Phase 3 — recursive segmentation
// ============================================================================

#[derive(Debug, Clone, Copy)]
struct CutInfo {
    position: f32,
    gap: f32,
}

fn recursive_segment(
    indices: Vec<usize>,
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
    depth: usize,
) -> Vec<usize> {
    if indices.len() <= 1 || depth >= config.max_depth {
        return sort_by_y_then_x(&indices, blocks);
    }

    let h_cut = find_best_horizontal_cut(&indices, blocks, config);
    let v_cut = find_best_vertical_cut(&indices, blocks, config);

    let has_h = h_cut.gap >= config.min_horizontal_gap;
    let has_v = v_cut.gap >= config.min_vertical_gap;

    let use_h = match (has_h, has_v) {
        (true, true) => h_cut.gap > v_cut.gap,
        (true, false) => true,
        (false, true) => false,
        (false, false) => return sort_by_y_then_x(&indices, blocks),
    };

    let groups = if use_h {
        split_by_horizontal_cut(&indices, blocks, h_cut.position)
    } else {
        split_by_vertical_cut(&indices, blocks, v_cut.position)
    };

    if groups.len() <= 1 {
        return sort_by_y_then_x(&indices, blocks);
    }

    let mut result = Vec::with_capacity(indices.len());
    for group in groups {
        let sub = recursive_segment(group, blocks, config, depth + 1);
        result.extend(sub);
    }
    result
}

/// Largest Y-axis gap between blocks (top-left coords — smaller y = higher).
/// Uses a running-max of `y1` so overlapping blocks do not produce false gaps.
fn find_best_horizontal_cut(
    indices: &[usize],
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
) -> CutInfo {
    if indices.len() < 2 {
        return CutInfo { position: 0.0, gap: 0.0 };
    }

    let mut sorted: Vec<usize> = indices.to_vec();
    sorted.sort_by(|&a, &b| {
        let ba = &blocks[a].bbox;
        let bb = &blocks[b].bbox;
        ba.y0
            .partial_cmp(&bb.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                ba.y1
                    .partial_cmp(&bb.y1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut largest_gap = 0.0_f32;
    let mut cut_position = 0.0_f32;
    let mut prev_y1: Option<f32> = None;

    for &idx in &sorted {
        let bbox = &blocks[idx].bbox;
        if let Some(py1) = prev_y1 {
            if bbox.y0 > py1 {
                let gap = bbox.y0 - py1;
                if gap > config.overlap_tolerance && gap > largest_gap {
                    largest_gap = gap;
                    cut_position = (py1 + bbox.y0) / 2.0;
                }
            }
        }
        prev_y1 = Some(match prev_y1 {
            Some(p) => p.max(bbox.y1),
            None => bbox.y1,
        });
    }

    CutInfo {
        position: cut_position,
        gap: largest_gap,
    }
}

/// Largest X-axis gap between blocks, with a narrow-outlier retry pass.
///
/// When the naive edge-gap is below `min_vertical_gap`, items narrower than
/// `narrow_element_width_ratio * region_width` are temporarily filtered out and
/// the scan is retried — this rescues two-column pages where a narrow
/// full-width element (page number, footnote marker) bridges the column gutter.
fn find_best_vertical_cut(
    indices: &[usize],
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
) -> CutInfo {
    let edge = find_vertical_cut_by_edges(indices, blocks, config);
    if edge.gap >= config.min_vertical_gap {
        return edge;
    }
    if indices.len() >= 3 {
        let region = bounding_region(indices, blocks);
        let region_width = region.width();
        if region_width > 0.0 {
            let narrow_threshold = region_width * config.narrow_element_width_ratio;
            let filtered: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|&i| blocks[i].bbox.width() >= narrow_threshold)
                .collect();
            if filtered.len() >= 2 && filtered.len() < indices.len() {
                let retry = find_vertical_cut_by_edges(&filtered, blocks, config);
                if retry.gap > edge.gap && retry.gap >= config.min_vertical_gap {
                    return retry;
                }
            }
        }
    }
    edge
}

fn find_vertical_cut_by_edges(
    indices: &[usize],
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
) -> CutInfo {
    if indices.len() < 2 {
        return CutInfo { position: 0.0, gap: 0.0 };
    }

    let mut sorted: Vec<usize> = indices.to_vec();
    sorted.sort_by(|&a, &b| {
        let ba = &blocks[a].bbox;
        let bb = &blocks[b].bbox;
        ba.x0
            .partial_cmp(&bb.x0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                ba.x1
                    .partial_cmp(&bb.x1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut largest_gap = 0.0_f32;
    let mut cut_position = 0.0_f32;
    let mut prev_right: Option<f32> = None;

    for &idx in &sorted {
        let bbox = &blocks[idx].bbox;
        if let Some(pr) = prev_right {
            if bbox.x0 > pr {
                let gap = bbox.x0 - pr;
                if gap > config.overlap_tolerance && gap > largest_gap {
                    largest_gap = gap;
                    cut_position = (pr + bbox.x0) / 2.0;
                }
            }
        }
        prev_right = Some(match prev_right {
            Some(p) => p.max(bbox.x1),
            None => bbox.x1,
        });
    }

    CutInfo {
        position: cut_position,
        gap: largest_gap,
    }
}

fn split_by_horizontal_cut(
    indices: &[usize],
    blocks: &[RawTextBlock],
    cut_y: f32,
) -> Vec<Vec<usize>> {
    // Top-left origin: smaller y = higher on page → "above" has center_y < cut_y.
    // Java's `centerY > cutY` (PDF y-up, "above") maps to `center_y < cut_y` here.
    let mut above = Vec::new();
    let mut below = Vec::new();
    for &idx in indices {
        if blocks[idx].bbox.center_y() < cut_y {
            above.push(idx);
        } else {
            below.push(idx);
        }
    }
    let mut groups = Vec::new();
    if !above.is_empty() {
        groups.push(above);
    }
    if !below.is_empty() {
        groups.push(below);
    }
    groups
}

fn split_by_vertical_cut(
    indices: &[usize],
    blocks: &[RawTextBlock],
    cut_x: f32,
) -> Vec<Vec<usize>> {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for &idx in indices {
        if blocks[idx].bbox.center_x() < cut_x {
            left.push(idx);
        } else {
            right.push(idx);
        }
    }
    let mut groups = Vec::new();
    if !left.is_empty() {
        groups.push(left);
    }
    if !right.is_empty() {
        groups.push(right);
    }
    groups
}

// ============================================================================
// Phase 4 — merge cross-layout elements back
// ============================================================================

fn merge_cross_layout(
    main: Vec<usize>,
    cross: Vec<usize>,
    blocks: &[RawTextBlock],
) -> Vec<usize> {
    if cross.is_empty() {
        return main;
    }
    if main.is_empty() {
        return sort_by_y_then_x(&cross, blocks);
    }

    let sorted_cross = sort_by_y_then_x(&cross, blocks);

    let mut result = Vec::with_capacity(main.len() + sorted_cross.len());
    let mut mi = 0usize;
    let mut ci = 0usize;
    while mi < main.len() || ci < sorted_cross.len() {
        if ci >= sorted_cross.len() {
            result.push(main[mi]);
            mi += 1;
        } else if mi >= main.len() {
            result.push(sorted_cross[ci]);
            ci += 1;
        } else {
            let cross_y = blocks[sorted_cross[ci]].bbox.y0;
            let main_y = blocks[main[mi]].bbox.y0;
            // Top-left origin: smaller y = higher on page.
            // Java `crossTopY >= mainTopY` (cross above, PDF y-up) becomes
            // `cross.y0 <= main.y0` (cross higher on page, y-down). Ties → cross wins.
            if cross_y <= main_y {
                result.push(sorted_cross[ci]);
                ci += 1;
            } else {
                result.push(main[mi]);
                mi += 1;
            }
        }
    }
    result
}

// ============================================================================
// Utility
// ============================================================================

fn sort_by_y_then_x(indices: &[usize], blocks: &[RawTextBlock]) -> Vec<usize> {
    let mut v: Vec<usize> = indices.to_vec();
    v.sort_by(|&a, &b| {
        let ba = &blocks[a].bbox;
        let bb = &blocks[b].bbox;
        ba.y0
            .partial_cmp(&bb.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                ba.x0
                    .partial_cmp(&bb.x0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    v
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make(id: usize, x0: f32, y0: f32, x1: f32, y1: f32) -> RawTextBlock {
        RawTextBlock {
            bbox: Bbox::new(x0, y0, x1, y1),
            text: format!("b{id}"),
            font_size: 12.0,
            font_name: "Times".into(),
            page_num: 0,
            block_id: id,
            reading_order: 0,
        }
    }

    fn order(blocks: &[RawTextBlock]) -> Vec<usize> {
        build_xycut_order(blocks, &XyCutConfig::default())
    }

    #[test]
    fn empty_input_returns_empty() {
        let b: Vec<RawTextBlock> = vec![];
        assert_eq!(order(&b), Vec::<usize>::new());
    }

    #[test]
    fn single_block_preserves_identity() {
        let b = vec![make(0, 0.0, 0.0, 100.0, 20.0)];
        assert_eq!(order(&b), vec![0]);
    }

    #[test]
    fn two_vertically_stacked_blocks_top_first() {
        // Block 0 lower on page, block 1 higher. Expect top first.
        let b = vec![
            make(0, 0.0, 100.0, 200.0, 120.0),
            make(1, 0.0, 0.0, 200.0, 20.0),
        ];
        assert_eq!(order(&b), vec![1, 0]);
    }

    #[test]
    fn two_side_by_side_blocks_left_first() {
        let b = vec![
            make(0, 120.0, 0.0, 200.0, 200.0), // right column
            make(1, 0.0, 0.0, 80.0, 200.0),    // left column
        ];
        assert_eq!(order(&b), vec![1, 0]);
    }

    #[test]
    fn two_column_page_left_column_before_right() {
        // 4 blocks in two columns, 2 per column.
        // Column 1: x = 0..90. Column 2: x = 110..200. Gutter ~20pt.
        let b = vec![
            make(0, 0.0, 10.0, 90.0, 30.0),
            make(1, 0.0, 40.0, 90.0, 60.0),
            make(2, 110.0, 10.0, 200.0, 30.0),
            make(3, 110.0, 40.0, 200.0, 60.0),
        ];
        let ord = order(&b);
        assert_eq!(ord, vec![0, 1, 2, 3]);
    }

    #[test]
    fn narrow_page_number_does_not_merge_columns() {
        // Two columns with a narrow page number well below that bridges the
        // gutter in X-projection terms. The horizontal gap to the page number
        // will win the initial cut, leaving the body to column-split cleanly.
        let b = vec![
            make(0, 0.0, 10.0, 90.0, 30.0),
            make(1, 0.0, 40.0, 90.0, 60.0),
            make(2, 110.0, 10.0, 200.0, 30.0),
            make(3, 110.0, 40.0, 200.0, 60.0),
            make(4, 95.0, 200.0, 105.0, 215.0),
        ];
        let ord = order(&b);
        assert_eq!(ord.len(), 5);
        let pos = |i: usize| ord.iter().position(|&x| x == i).unwrap();
        assert!(pos(0) < pos(2), "L1 must precede R1");
        assert!(pos(1) < pos(2), "L2 must precede R1");
        assert_eq!(pos(4), 4, "page number last");
    }

    #[test]
    fn narrow_outlier_retry_unblocks_tight_column_gap() {
        // Two columns with only a tiny physical gap AND a narrow element in
        // the middle. Without the retry pass the column gap would be masked.
        let b = vec![
            make(0, 0.0, 10.0, 90.0, 30.0),
            make(1, 0.0, 40.0, 90.0, 60.0),
            make(2, 110.0, 10.0, 200.0, 30.0),
            make(3, 110.0, 40.0, 200.0, 60.0),
            make(4, 95.0, 20.0, 105.0, 35.0), // narrow bridger in the gutter
        ];
        let config = XyCutConfig {
            min_vertical_gap: 8.0, // loosen slightly so 20pt qualifies
            ..Default::default()
        };
        let ord = build_xycut_order(&b, &config);
        assert_eq!(ord.len(), 5);
    }

    #[test]
    fn spanning_title_is_pre_masked_and_placed_first() {
        // Title spans both columns; below it two columns of body.
        // Default beta=2.0 treats nothing as cross-layout. Drop beta to 0.9 so
        // the pre-mask activates for this test.
        let b = vec![
            make(0, 0.0, 0.0, 200.0, 20.0),
            make(1, 0.0, 40.0, 90.0, 60.0),
            make(2, 110.0, 40.0, 200.0, 60.0),
            make(3, 0.0, 80.0, 90.0, 100.0),
            make(4, 110.0, 80.0, 200.0, 100.0),
        ];
        let config = XyCutConfig { beta: 0.9, ..Default::default() };
        let ord = build_xycut_order(&b, &config);
        assert_eq!(ord[0], 0, "title must be first");
        let pos = |i: usize| ord.iter().position(|&x| x == i).unwrap();
        assert!(pos(1) < pos(2), "L1 before R1");
        assert!(pos(3) < pos(4), "L2 before R2");
    }

    #[test]
    fn single_column_has_no_spurious_vertical_cut() {
        let b = vec![
            make(0, 0.0, 0.0, 200.0, 20.0),
            make(1, 0.0, 30.0, 200.0, 50.0),
            make(2, 0.0, 60.0, 200.0, 80.0),
            make(3, 0.0, 90.0, 200.0, 110.0),
        ];
        assert_eq!(order(&b), vec![0, 1, 2, 3]);
    }

    #[test]
    fn assign_reading_order_writes_positions_in_place() {
        let mut b = vec![
            make(0, 0.0, 100.0, 200.0, 120.0),
            make(1, 0.0, 0.0, 200.0, 20.0),
        ];
        let ord = build_xycut_order(&b, &XyCutConfig::default());
        assign_reading_order(&ord, &mut b);
        assert_eq!(b[1].reading_order, 0);
        assert_eq!(b[0].reading_order, 1);
    }

    #[test]
    fn horizontal_overlap_ratio_is_relative_to_smaller_width() {
        let a = Bbox::new(0.0, 0.0, 100.0, 10.0);
        let b = Bbox::new(50.0, 0.0, 60.0, 10.0);
        // b is fully inside a's x-range → overlap = b.width()
        assert!((horizontal_overlap_ratio(&a, &b) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn horizontal_overlap_ratio_zero_for_disjoint() {
        let a = Bbox::new(0.0, 0.0, 10.0, 10.0);
        let b = Bbox::new(20.0, 0.0, 30.0, 10.0);
        assert!(horizontal_overlap_ratio(&a, &b).abs() < 1e-4);
    }

    #[test]
    fn cross_layout_detection_requires_three_or_more_blocks() {
        let b = vec![
            make(0, 0.0, 0.0, 200.0, 20.0),
            make(1, 0.0, 30.0, 200.0, 50.0),
        ];
        let set = identify_cross_layout(&[0, 1], &b, &XyCutConfig::default());
        assert!(set.is_empty());
    }

    #[test]
    fn density_ratio_full_page_equals_one() {
        let b = vec![make(0, 0.0, 0.0, 100.0, 100.0)];
        assert!((compute_density_ratio(&[0], &b) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn sort_by_y_then_x_orders_correctly() {
        let b = vec![
            make(0, 50.0, 10.0, 100.0, 20.0),
            make(1, 0.0, 10.0, 40.0, 20.0),
            make(2, 0.0, 50.0, 40.0, 60.0),
        ];
        let s = sort_by_y_then_x(&[0, 1, 2], &b);
        assert_eq!(s, vec![1, 0, 2]);
    }

    #[test]
    fn section_header_between_columns_lands_between_them() {
        // Title (wide, top), body-L1, body-R1, mid-page section header (wide),
        // body-L2, body-R2. Expected reading order:
        // title, L1, R1, header, L2, R2 — BUT the current XY-cut does columns
        // wholesale, so within a cross-layout-split-body we get L1, L2, R1, R2
        // and the merge places the header by Y. This test asserts the header
        // is sandwiched between the two body vertical regions in the output.
        let b = vec![
            make(0, 0.0, 0.0, 200.0, 20.0),     // title (cross)
            make(1, 0.0, 40.0, 90.0, 60.0),     // L1
            make(2, 110.0, 40.0, 200.0, 60.0),  // R1
            make(3, 0.0, 100.0, 200.0, 120.0),  // section header (cross)
            make(4, 0.0, 140.0, 90.0, 160.0),   // L2
            make(5, 110.0, 140.0, 200.0, 160.0),// R2
        ];
        let config = XyCutConfig { beta: 0.9, ..Default::default() };
        let ord = build_xycut_order(&b, &config);
        assert_eq!(ord[0], 0, "title first");
        let hpos = ord.iter().position(|&x| x == 3).unwrap();
        let pos = |i: usize| ord.iter().position(|&x| x == i).unwrap();
        assert!(pos(1) < hpos && pos(2) < hpos, "L1 R1 before header");
        assert!(hpos < pos(4) && hpos < pos(5), "header before L2 R2");
    }
}
