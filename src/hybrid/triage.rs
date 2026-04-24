//! Per-page triage: decide whether a page should be routed through the
//! external hybrid backend (Docling) or kept on the fast local path.
//!
//! Current heuristics, any of which trips the page into routing:
//!
//! 1. **Math density** — the page contains at least `MATH_SYMBOL_THRESHOLD`
//!    characters drawn from a curated Unicode math/greek set. These are the
//!    pages most likely to lose information on the local path because math
//!    fonts frequently ship without a ToUnicode mapping, so glyphs silently
//!    vanish.
//! 2. **Table present** — the page has at least one `BlockKind::TableCell`.
//!    The local table detector is a bbox-grid clusterer; it reconstructs
//!    simple tables adequately but mangles anything with merged cells,
//!    multi-line cells, or right-aligned numeric columns. Routing such
//!    pages gets us Docling's TableFormer output.
//! 3. **Low text density** — very few extractable text blocks relative to
//!    page area suggests the page is a scanned image. Docling's OCR path
//!    turns such a page from blank into readable.

use crate::document::types::{Block, BlockKind, Page};

/// Curated Unicode ranges of math-looking characters. Covers Mathematical
/// Operators, Supplemental Math Operators, Letterlike Symbols, arrows and
/// Greek letters commonly used in formulas.
#[inline]
fn is_math_char(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        0x03B1..=0x03C9        // Greek small letters (α-ω)
        | 0x0391..=0x03A9      // Greek capital letters (Α-Ω)
        | 0x2100..=0x214F      // Letterlike Symbols (ℕ ℤ ℚ ℝ ℂ ℏ ℵ …)
        | 0x2190..=0x21FF      // Arrows (← → ↔ ⇒ ⇔ …)
        | 0x2200..=0x22FF      // Mathematical Operators (∀ ∃ ∈ ∉ ⊂ ⊃ ∑ ∏ ∫ ∂ ∇ ⊕ ⊗ …)
        | 0x27C0..=0x27EF      // Miscellaneous Math A (⟨ ⟩ ⟦ ⟧ …)
        | 0x2A00..=0x2AFF      // Supplemental Math Operators (⨂ ⨁ ⨆ …)
    )
}

/// Minimum math-char count on a page to trip the math-density heuristic.
pub const MATH_SYMBOL_THRESHOLD: usize = 4;

/// Minimum text-area fraction (relative to page area) below which the page
/// is considered "likely scanned". A page with a lot of area and no
/// text blocks ends up below this.
const MIN_TEXT_AREA_FRACTION: f32 = 0.02;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanReport {
    pub pages_total: usize,
    pub pages_with_readable_text: usize,
    pub image_only_pages: usize,
    pub low_density_pages: usize,
}

impl ScanReport {
    pub fn likely_scan_like(self) -> bool {
        if self.pages_total == 0 {
            return false;
        }
        if self.image_only_pages == self.pages_total {
            return true;
        }
        self.pages_with_readable_text == 0 && self.low_density_pages > 0
    }
}

/// Should this page be routed through the hybrid backend?
pub fn should_route(page: &Page) -> bool {
    has_table(page) || is_math_heavy(page) || is_low_density(page)
}

pub fn has_table(page: &Page) -> bool {
    page.blocks
        .iter()
        .any(|b| matches!(b.kind, BlockKind::TableCell { .. }))
}

pub fn is_math_heavy(page: &Page) -> bool {
    count_math_chars(&page.blocks) >= MATH_SYMBOL_THRESHOLD
}

pub fn is_low_density(page: &Page) -> bool {
    // Pure empty page → yes (likely scanned).
    if page.blocks.is_empty() {
        return true;
    }
    let page_area = (page.width * page.height).max(1.0);
    let text_area: f32 = page
        .blocks
        .iter()
        .filter(|b| {
            !matches!(
                b.kind,
                BlockKind::Image { .. }
                    | BlockKind::Figure { .. }
                    | BlockKind::PageNumber
                    | BlockKind::RunningHeader
                    | BlockKind::RunningFooter
            )
        })
        .map(|b| b.bbox.area())
        .sum();
    (text_area / page_area) < MIN_TEXT_AREA_FRACTION
}

pub fn has_readable_text(page: &Page) -> bool {
    page.blocks.iter().any(block_counts_as_readable_text)
}

pub fn is_image_only(page: &Page) -> bool {
    !page.blocks.is_empty()
        && page
            .blocks
            .iter()
            .all(|b| matches!(b.kind, BlockKind::Image { .. } | BlockKind::Figure { .. }))
}

pub fn scan_report(pages: &[Page]) -> ScanReport {
    let mut report = ScanReport {
        pages_total: pages.len(),
        ..Default::default()
    };

    for page in pages {
        if has_readable_text(page) {
            report.pages_with_readable_text += 1;
        }
        if is_image_only(page) {
            report.image_only_pages += 1;
        }
        if is_low_density(page) {
            report.low_density_pages += 1;
        }
    }

    report
}

fn count_math_chars(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .filter(|b| {
            !matches!(
                b.kind,
                BlockKind::Image { .. }
                    | BlockKind::Figure { .. }
                    | BlockKind::PageNumber
                    | BlockKind::RunningHeader
                    | BlockKind::RunningFooter
            )
        })
        .flat_map(|b| b.text.chars())
        .filter(|c| is_math_char(*c))
        .count()
}

fn block_counts_as_readable_text(block: &Block) -> bool {
    match &block.kind {
        BlockKind::Paragraph
        | BlockKind::Heading { .. }
        | BlockKind::ListItem { .. }
        | BlockKind::TableCell { .. }
        | BlockKind::Caption
        | BlockKind::CodeBlock
        | BlockKind::Formula { .. } => {
            !block.text.trim().is_empty() || matches!(block.kind, BlockKind::Formula { .. })
        }
        BlockKind::Figure { caption, .. } => {
            caption.as_deref().is_some_and(|c| !c.trim().is_empty())
        }
        BlockKind::PageNumber
        | BlockKind::RunningHeader
        | BlockKind::RunningFooter
        | BlockKind::Image { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::types::Bbox;

    fn block(kind: BlockKind, text: &str, bbox: Bbox) -> Block {
        Block {
            id: 0,
            bbox,
            text: text.to_string(),
            kind,
            font_size: 12.0,
            font_name: "Times".to_string(),
            page_num: 0,
            reading_order: 0,
        }
    }

    fn page(blocks: Vec<Block>, width: f32, height: f32) -> Page {
        Page {
            page_num: 0,
            width,
            height,
            blocks,
            override_markdown: None,
        }
    }

    #[test]
    fn math_heavy_page_gets_routed() {
        let p = page(
            vec![block(
                BlockKind::Paragraph,
                "Let f(x) = ∫ g(x) dx where ∂g/∂x ∈ ℝ and ∀x ∈ ℕ",
                Bbox::new(0.0, 0.0, 500.0, 200.0),
            )],
            612.0,
            792.0,
        );
        assert!(is_math_heavy(&p));
        assert!(should_route(&p));
    }

    #[test]
    fn plain_prose_is_not_routed() {
        let text = "This is a paragraph of plain English prose with no \
                    mathematical symbols, just ordinary sentences. The \
                    content is descriptive and expository.";
        let p = page(
            vec![block(
                BlockKind::Paragraph,
                text,
                Bbox::new(0.0, 0.0, 500.0, 200.0),
            )],
            612.0,
            792.0,
        );
        assert!(!is_math_heavy(&p));
        assert!(!has_table(&p));
        assert!(!is_low_density(&p));
        assert!(!should_route(&p));
    }

    #[test]
    fn table_present_is_routed() {
        let p = page(
            vec![
                block(
                    BlockKind::TableCell { row: 0, col: 0 },
                    "header",
                    Bbox::new(0.0, 0.0, 100.0, 20.0),
                ),
                block(
                    BlockKind::TableCell { row: 1, col: 0 },
                    "cell",
                    Bbox::new(0.0, 20.0, 100.0, 40.0),
                ),
            ],
            612.0,
            792.0,
        );
        assert!(has_table(&p));
        assert!(should_route(&p));
    }

    #[test]
    fn empty_page_is_low_density() {
        let p = page(vec![], 612.0, 792.0);
        assert!(is_low_density(&p));
        assert!(should_route(&p));
    }

    #[test]
    fn nearly_empty_page_is_low_density() {
        // Page with only a page-number at the bottom — likely a scan
        let p = page(
            vec![block(
                BlockKind::PageNumber,
                "3",
                Bbox::new(280.0, 780.0, 320.0, 790.0),
            )],
            612.0,
            792.0,
        );
        assert!(is_low_density(&p));
    }

    #[test]
    fn image_only_page_does_not_trigger_low_density() {
        // Image blocks do not count toward text density, so an image-only
        // page is low-density by default. That's fine — routing it gets us
        // captions/OCR from Docling.
        let p = page(
            vec![block(
                BlockKind::Image {
                    path: Some("images/img.png".to_string()),
                },
                "",
                Bbox::new(0.0, 0.0, 600.0, 400.0),
            )],
            612.0,
            792.0,
        );
        assert!(is_low_density(&p));
    }

    #[test]
    fn image_only_page_is_not_readable_text() {
        let p = page(
            vec![block(
                BlockKind::Image {
                    path: Some("images/img.png".to_string()),
                },
                "",
                Bbox::new(0.0, 0.0, 600.0, 400.0),
            )],
            612.0,
            792.0,
        );
        assert!(is_image_only(&p));
        assert!(!has_readable_text(&p));
    }

    #[test]
    fn scan_report_flags_fully_image_only_document() {
        let pages = vec![
            page(
                vec![block(
                    BlockKind::Image {
                        path: Some("images/p1.png".to_string()),
                    },
                    "",
                    Bbox::new(0.0, 0.0, 500.0, 700.0),
                )],
                612.0,
                792.0,
            ),
            page(
                vec![block(
                    BlockKind::Image {
                        path: Some("images/p2.png".to_string()),
                    },
                    "",
                    Bbox::new(0.0, 0.0, 500.0, 700.0),
                )],
                612.0,
                792.0,
            ),
        ];
        let report = scan_report(&pages);
        assert_eq!(report.image_only_pages, 2);
        assert_eq!(report.pages_with_readable_text, 0);
        assert!(report.likely_scan_like());
    }

    #[test]
    fn scan_report_does_not_flag_normal_prose_document() {
        let pages = vec![page(
            vec![block(
                BlockKind::Paragraph,
                "Readable prose on a normal page.",
                Bbox::new(0.0, 0.0, 500.0, 300.0),
            )],
            612.0,
            792.0,
        )];
        let report = scan_report(&pages);
        assert_eq!(report.pages_with_readable_text, 1);
        assert!(!report.likely_scan_like());
    }

    #[test]
    fn math_threshold_requires_at_least_four_symbols() {
        // Three symbols → not routed on math grounds alone.
        let p = page(
            vec![block(
                BlockKind::Paragraph,
                "Simple prose with ∞ and ∑ and α and nothing else.",
                Bbox::new(0.0, 0.0, 500.0, 200.0),
            )],
            612.0,
            792.0,
        );
        let count = count_math_chars(&p.blocks);
        assert!(count >= 3, "expected at least 3 math chars, got {count}");
    }

    #[test]
    fn running_footer_text_is_excluded_from_math_count() {
        let p = page(
            vec![
                block(
                    BlockKind::Paragraph,
                    "Plain prose.",
                    Bbox::new(0.0, 0.0, 500.0, 200.0),
                ),
                block(
                    BlockKind::RunningFooter,
                    "∫ ∑ ∏ ∀ ∃",
                    Bbox::new(0.0, 780.0, 500.0, 792.0),
                ),
            ],
            612.0,
            792.0,
        );
        // Math only appears in the footer, which is excluded → not math-heavy.
        assert!(!is_math_heavy(&p));
    }
}
