use crate::document::types::RawTextBlock;

/// Configuration for the XY-Cut++ algorithm.
#[derive(Debug, Clone)]
pub struct XyCutConfig {
    /// Minimum vertical whitespace gap to make a horizontal cut (points). Default: 8.0
    pub min_horizontal_gap: f32,
    /// Minimum horizontal whitespace gap to make a vertical cut (points). Default: 12.0
    pub min_vertical_gap: f32,
    /// Blocks overlapping by less than this are treated as non-overlapping (points). Default: 2.0
    pub overlap_tolerance: f32,
    /// Maximum recursion depth guard. Default: 50
    pub max_depth: usize,
}

impl Default for XyCutConfig {
    fn default() -> Self {
        Self {
            min_horizontal_gap: 8.0,
            min_vertical_gap: 12.0,
            overlap_tolerance: 2.0,
            max_depth: 50,
        }
    }
}

/// A node in the XY-Cut tree.
#[derive(Debug)]
pub enum XyCutNode {
    Leaf {
        /// Indices into the original blocks slice, in top-to-bottom order within the leaf.
        block_indices: Vec<usize>,
    },
    HorizontalCut {
        top: Box<XyCutNode>,
        bottom: Box<XyCutNode>,
    },
    VerticalCut {
        left: Box<XyCutNode>,
        right: Box<XyCutNode>,
    },
}

/// Build the XY-Cut++ tree for a set of blocks on a page.
/// Returns a tree whose in-order traversal gives reading order.
///
/// TODO: wire `merge_fragmented_words` output back into gap-detection for the full
/// "++" improvement. Currently the merge is a no-op with respect to the final tree.
pub fn build_xycut_tree(blocks: &[RawTextBlock], config: &XyCutConfig) -> XyCutNode {
    let all_indices: Vec<usize> = (0..blocks.len()).collect();
    build_region(&all_indices, blocks, config, 0)
}

/// Assign `reading_order` to blocks by doing an in-order traversal of the tree.
pub fn assign_reading_order(tree: &XyCutNode, blocks: &mut Vec<RawTextBlock>) {
    let mut counter = 0usize;
    traverse(tree, blocks, &mut counter);
}

fn traverse(node: &XyCutNode, blocks: &mut Vec<RawTextBlock>, counter: &mut usize) {
    match node {
        XyCutNode::Leaf { block_indices } => {
            // Within a leaf, sort by y0 then x0
            let mut sorted = block_indices.clone();
            sorted.sort_by(|&a, &b| {
                let ba = &blocks[a].bbox;
                let bb = &blocks[b].bbox;
                ba.y0
                    .partial_cmp(&bb.y0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(ba.x0.partial_cmp(&bb.x0).unwrap_or(std::cmp::Ordering::Equal))
            });
            for idx in sorted {
                blocks[idx].reading_order = *counter;
                *counter += 1;
            }
        }
        XyCutNode::HorizontalCut { top, bottom } => {
            traverse(top, blocks, counter);
            traverse(bottom, blocks, counter);
        }
        XyCutNode::VerticalCut { left, right } => {
            traverse(left, blocks, counter);
            traverse(right, blocks, counter);
        }
    }
}

fn build_region(
    indices: &[usize],
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
    depth: usize,
) -> XyCutNode {
    if indices.len() <= 1 || depth >= config.max_depth {
        return XyCutNode::Leaf {
            block_indices: indices.to_vec(),
        };
    }

    // Try horizontal cut first (top-to-bottom takes priority in reading order)
    if let Some(cut_y) = best_horizontal_cut(indices, blocks, config) {
        let (top_idx, bottom_idx) = split_horizontal(indices, blocks, cut_y);
        if !top_idx.is_empty() && !bottom_idx.is_empty() {
            return XyCutNode::HorizontalCut {
                top: Box::new(build_region(&top_idx, blocks, config, depth + 1)),
                bottom: Box::new(build_region(&bottom_idx, blocks, config, depth + 1)),
            };
        }
    }

    // Try vertical cut (columns)
    if let Some(cut_x) = best_vertical_cut(indices, blocks, config) {
        let (left_idx, right_idx) = split_vertical(indices, blocks, cut_x);
        if !left_idx.is_empty() && !right_idx.is_empty() {
            return XyCutNode::VerticalCut {
                left: Box::new(build_region(&left_idx, blocks, config, depth + 1)),
                right: Box::new(build_region(&right_idx, blocks, config, depth + 1)),
            };
        }
    }

    // No cut possible — leaf
    XyCutNode::Leaf {
        block_indices: indices.to_vec(),
    }
}

/// Find the y-coordinate of the best horizontal cut (largest vertical gap between blocks).
/// Returns `None` if no gap exceeds `min_horizontal_gap`.
fn best_horizontal_cut(
    indices: &[usize],
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
) -> Option<f32> {
    let mut sorted = indices.to_vec();
    sorted.sort_by(|&a, &b| {
        blocks[a]
            .bbox
            .y0
            .partial_cmp(&blocks[b].bbox.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut best_gap = config.min_horizontal_gap;
    let mut best_cut = None;
    let mut max_y1_so_far = f32::NEG_INFINITY;

    for &idx in &sorted {
        let bbox = &blocks[idx].bbox;
        let gap = bbox.y0 - max_y1_so_far;

        // Apply overlap tolerance: treat small overlaps as zero gap; skip large overlaps
        let effective_gap = if gap >= -config.overlap_tolerance {
            gap.max(0.0)
        } else {
            if bbox.y1 > max_y1_so_far {
                max_y1_so_far = bbox.y1;
            }
            continue;
        };

        if effective_gap > best_gap {
            best_gap = effective_gap;
            best_cut = Some(max_y1_so_far + effective_gap / 2.0);
        }

        if bbox.y1 > max_y1_so_far {
            max_y1_so_far = bbox.y1;
        }
    }

    best_cut
}

/// Find the x-coordinate of the best vertical cut (largest horizontal gap between blocks).
/// Returns `None` if no gap exceeds `min_vertical_gap`.
fn best_vertical_cut(
    indices: &[usize],
    blocks: &[RawTextBlock],
    config: &XyCutConfig,
) -> Option<f32> {
    let mut sorted = indices.to_vec();
    sorted.sort_by(|&a, &b| {
        blocks[a]
            .bbox
            .x0
            .partial_cmp(&blocks[b].bbox.x0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut best_gap = config.min_vertical_gap;
    let mut best_cut = None;
    let mut max_x1_so_far = f32::NEG_INFINITY;

    for &idx in &sorted {
        let bbox = &blocks[idx].bbox;
        let gap = bbox.x0 - max_x1_so_far;

        let effective_gap = if gap >= -config.overlap_tolerance {
            gap.max(0.0)
        } else {
            if bbox.x1 > max_x1_so_far {
                max_x1_so_far = bbox.x1;
            }
            continue;
        };

        if effective_gap > best_gap {
            best_gap = effective_gap;
            best_cut = Some(max_x1_so_far + effective_gap / 2.0);
        }

        if bbox.x1 > max_x1_so_far {
            max_x1_so_far = bbox.x1;
        }
    }

    best_cut
}

fn split_horizontal(
    indices: &[usize],
    blocks: &[RawTextBlock],
    cut_y: f32,
) -> (Vec<usize>, Vec<usize>) {
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    for &idx in indices {
        if blocks[idx].bbox.center_y() <= cut_y {
            top.push(idx);
        } else {
            bottom.push(idx);
        }
    }
    (top, bottom)
}

fn split_vertical(
    indices: &[usize],
    blocks: &[RawTextBlock],
    cut_x: f32,
) -> (Vec<usize>, Vec<usize>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for &idx in indices {
        if blocks[idx].bbox.center_x() <= cut_x {
            left.push(idx);
        } else {
            right.push(idx);
        }
    }
    (left, right)
}

/// "++" improvement: merge text blocks on the same baseline with tiny horizontal gaps.
/// Prevents a single word split across two mupdf blocks from being treated as separate
/// regions. Returns a new vec of merged blocks for use in gap-detection pre-processing.
///
/// TODO: feed the merged vec back into `build_region` gap-detection while keeping the
/// original block indices intact for `assign_reading_order`.
#[allow(dead_code)]
fn merge_fragmented_words(blocks: &[RawTextBlock]) -> Vec<RawTextBlock> {
    const H_MERGE_THRESHOLD: f32 = 2.0;
    const V_MERGE_THRESHOLD: f32 = 2.0;

    if blocks.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<&RawTextBlock> = blocks.iter().collect();
    sorted.sort_by(|a, b| {
        a.bbox
            .y0
            .partial_cmp(&b.bbox.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.bbox.x0.partial_cmp(&b.bbox.x0).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut result: Vec<RawTextBlock> = Vec::new();
    for block in sorted {
        if let Some(last) = result.last_mut() {
            let y_overlap = (last.bbox.y0 - block.bbox.y0).abs() < V_MERGE_THRESHOLD
                || (last.bbox.y1 - block.bbox.y1).abs() < V_MERGE_THRESHOLD;
            let h_gap = block.bbox.x0 - last.bbox.x1;
            let same_font_size = (last.font_size - block.font_size).abs() < 0.5;

            if y_overlap && (0.0..=H_MERGE_THRESHOLD).contains(&h_gap) && same_font_size {
                last.bbox.x1 = last.bbox.x1.max(block.bbox.x1);
                last.bbox.y0 = last.bbox.y0.min(block.bbox.y0);
                last.bbox.y1 = last.bbox.y1.max(block.bbox.y1);
                if !last.text.is_empty() && !block.text.is_empty() {
                    last.text.push(' ');
                }
                last.text.push_str(&block.text);
                continue;
            }
        }
        result.push(block.clone());
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::types::Bbox;

    fn make_block(x0: f32, y0: f32, x1: f32, y1: f32, id: usize) -> RawTextBlock {
        RawTextBlock {
            bbox: Bbox::new(x0, y0, x1, y1),
            text: format!("block-{id}"),
            font_size: 12.0,
            font_name: "Times".to_string(),
            page_num: 0,
            block_id: id,
            reading_order: 0,
        }
    }

    #[test]
    fn single_block_is_leaf() {
        let blocks = vec![make_block(0.0, 0.0, 100.0, 20.0, 0)];
        let config = XyCutConfig::default();
        let tree = build_xycut_tree(&blocks, &config);
        assert!(matches!(tree, XyCutNode::Leaf { .. }));
    }

    #[test]
    fn two_vertically_separated_blocks_horizontal_cut() {
        // Block 0: top of page, Block 1: bottom with a large gap
        let blocks = vec![
            make_block(0.0, 0.0, 200.0, 20.0, 0),
            make_block(0.0, 50.0, 200.0, 70.0, 1),
        ];
        let config = XyCutConfig::default();
        let tree = build_xycut_tree(&blocks, &config);
        assert!(matches!(tree, XyCutNode::HorizontalCut { .. }));
    }

    #[test]
    fn two_side_by_side_blocks_vertical_cut() {
        // Block 0: left column, Block 1: right column, large horizontal gap
        let blocks = vec![
            make_block(0.0, 0.0, 80.0, 200.0, 0),
            make_block(120.0, 0.0, 200.0, 200.0, 1),
        ];
        let config = XyCutConfig::default();
        let tree = build_xycut_tree(&blocks, &config);
        assert!(matches!(tree, XyCutNode::VerticalCut { .. }));
    }

    #[test]
    fn reading_order_assigned_top_to_bottom() {
        let mut blocks = vec![
            make_block(0.0, 50.0, 200.0, 70.0, 0), // lower on page
            make_block(0.0, 0.0, 200.0, 20.0, 1),  // higher on page
        ];
        let config = XyCutConfig::default();
        let tree = build_xycut_tree(&blocks, &config);
        assign_reading_order(&tree, &mut blocks);
        // block at y0=0 should get order 0, block at y0=50 should get order 1
        assert_eq!(blocks[1].reading_order, 0); // block_id=1, higher up
        assert_eq!(blocks[0].reading_order, 1); // block_id=0, lower down
    }

    #[test]
    fn empty_blocks_returns_leaf() {
        let blocks: Vec<RawTextBlock> = vec![];
        let config = XyCutConfig::default();
        let tree = build_xycut_tree(&blocks, &config);
        assert!(matches!(tree, XyCutNode::Leaf { block_indices } if block_indices.is_empty()));
    }
}
