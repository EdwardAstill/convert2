use super::*;
use crate::document::types::{
    Bbox, Block, BlockKind, DetectedTable, Document, DocumentMetadata, Page, TableRender,
};
use crate::layout::table_inference::{
    normalize_numeric_row, strip_leading_list_marker, ParsedNumericRow,
};
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
        font_size,
        font_name: "Helvetica".to_string(),
        ..Block::text(id, bbox, text.to_string(), kind, 0, reading_order)
    }
}

fn make_doc(pages: Vec<Page>) -> Document {
    Document {
        source_path: PathBuf::from("test.pdf"),
        pages,
        metadata: DocumentMetadata::default(),
    }
}

fn make_page(page_num: usize, width: f32, height: f32, blocks: Vec<Block>) -> Page {
    Page {
        page_num,
        width,
        height,
        blocks,
        override_markdown: None,
    }
}

#[test]
fn renders_heading_paragraph() {
    let page = make_page(
        0,
        595.0,
        842.0,
        vec![
            make_block(0, "Introduction", BlockKind::Heading { level: 1 }, 0),
            make_block(1, "Hello world.", BlockKind::Paragraph, 1),
        ],
    );
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::faithful(false, None);
    let result = renderer.render_document(&doc).unwrap();
    assert!(result.markdown.contains("# Introduction"));
    assert!(result.markdown.contains("Hello world."));
}

#[test]
fn renders_formula_review_marker() {
    let page = Page {
        page_num: 0,
        width: 595.0,
        height: 842.0,
        blocks: vec![make_block(
            0,
            "",
            BlockKind::FormulaReview {
                reason: "visual-isolated-equation-band+cue:Hence:".into(),
                crop_path: Some("debug/formulas/page1_formula1.png".into()),
            },
            0,
        )],
        override_markdown: None,
    };
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::faithful(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(result.markdown.contains("<!-- formula-review: page=1"));
    assert!(result.markdown.contains("visual-isolated-equation-band"));
    assert!(result
        .markdown
        .contains("crop=\"debug/formulas/page1_formula1.png\""));
    assert!(!result.markdown.contains("$$"));
}

#[test]
fn renderer_new_uses_clean_default() {
    let page = make_page(
        0,
        595.0,
        842.0,
        vec![
            make_block(0, "Wrapped PDF", BlockKind::Paragraph, 0),
            make_block(1, "line.", BlockKind::Paragraph, 1),
        ],
    );
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::new(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(result.markdown.contains("Wrapped PDF line."));
}

#[test]
fn clean_style_suppresses_formula_review_marker() {
    let page = Page {
        page_num: 0,
        width: 595.0,
        height: 842.0,
        blocks: vec![make_block(
            0,
            "",
            BlockKind::FormulaReview {
                reason: "visual-isolated-equation-band".into(),
                crop_path: Some("debug/formulas/page1_formula1.png".into()),
            },
            0,
        )],
        override_markdown: None,
    };
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::clean(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(!result.markdown.contains("formula-review"));
    assert!(result.markdown.contains("<!-- page:1 -->"));
}

#[test]
fn clean_style_reflows_pdf_line_fragments() {
    let page = make_page(
        0,
        595.0,
        842.0,
        vec![
            make_block(
                0,
                "Lorem ipsum dolor sit amet, consectetur",
                BlockKind::Paragraph,
                0,
            ),
            make_block(1, "adipiscing elit.", BlockKind::Paragraph, 1),
            make_block(2, "Second paragraph.", BlockKind::Paragraph, 2),
        ],
    );
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::clean(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(result
        .markdown
        .contains("Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n\nSecond paragraph."));
}

#[test]
fn clean_style_normalizes_common_pdf_glyph_artifacts() {
    let page = make_page(
        0,
        595.0,
        842.0,
        vec![make_block(
            0,
            "\u{f0b7}f \u{fb01} \u{fb02} bad�text",
            BlockKind::Paragraph,
            0,
        )],
    );
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::clean(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(result.markdown.contains("- f fi fl badtext"));
}

#[test]
fn clean_style_preserves_fenced_code_spacing() {
    let page = make_page(
        0,
        595.0,
        842.0,
        vec![make_block(
            0,
            "A        B\n1        2",
            BlockKind::CodeBlock,
            0,
        )],
    );
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::clean(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(result.markdown.contains("```\nA        B\n1        2\n```"));
}

#[test]
fn renders_formula_with_latex_as_display_math() {
    let page = Page {
        page_num: 0,
        width: 595.0,
        height: 842.0,
        blocks: vec![make_block(
            0,
            "F = ma",
            BlockKind::Formula {
                latex: "F = ma".into(),
                display: true,
            },
            0,
        )],
        override_markdown: None,
    };
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::faithful(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(
        result.markdown.contains("$$ F = ma $$"),
        "display formula should emit $$ ... $$, got: {}",
        result.markdown
    );
}

#[test]
fn reflows_artificial_blank_lines_inside_paragraphs() {
    let page = Page {
        page_num: 0,
        width: 595.0,
        height: 842.0,
        blocks: vec![make_block(
            0,
            "This is one\n\nparagraph with\n\nartificial breaks.",
            BlockKind::Paragraph,
            0,
        )],
        override_markdown: None,
    };
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::faithful(false, None);
    let result = renderer.render_document(&doc).unwrap();
    assert!(result
        .markdown
        .contains("This is one paragraph with artificial breaks."));
}

#[test]
fn paragraph_reflow_preserves_mathish_subscript_and_superscript_markup() {
    let page = Page {
        page_num: 0,
        width: 595.0,
        height: 842.0,
        blocks: vec![make_block(
            0,
            "For x^{2}\n+\ny_{i}, the term remains stable.",
            BlockKind::Paragraph,
            0,
        )],
        override_markdown: None,
    };
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::faithful(false, None);
    let result = renderer.render_document(&doc).unwrap();
    assert!(result
        .markdown
        .contains("For x^{2} + y_{i}, the term remains stable."));
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
            make_block(2, "Tagged artifact", BlockKind::Artifact, 2),
            make_block(3, "Body text.", BlockKind::Paragraph, 3),
        ],
        override_markdown: None,
    };
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::faithful(false, None);
    let result = renderer.render_document(&doc).unwrap();
    assert!(!result.markdown.contains("Chapter 1"));
    assert!(!result.markdown.contains("Tagged artifact"));
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(result.markdown.contains("- Invoice Number: 2020-10"));
    assert!(result.markdown.contains("- Invoice Date: January 8, 2020"));
    assert!(!result.markdown.contains("| --- |"));
}

#[test]
fn clean_style_preserves_coordinate_table_line_breaks() {
    let page = Page {
        page_num: 0,
        width: 595.0,
        height: 842.0,
        blocks: vec![Block::special(
            10,
            Bbox::new(50.0, 50.0, 300.0, 120.0),
            BlockKind::CoordinateTable {
                table: DetectedTable {
                    bbox: Bbox::new(50.0, 50.0, 300.0, 120.0),
                    rows: vec![
                        vec!["Name".to_string(), "Age".to_string()],
                        vec!["Alice".to_string(), "30".to_string()],
                    ],
                    confidence: 0.9,
                    render: TableRender::Markdown,
                },
            },
            0,
            0.0,
            "table".to_string(),
        )],
        override_markdown: None,
    };
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::clean(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(result
        .markdown
        .contains("| Name | Age |\n| --- | --- |\n| Alice | 30 |"));
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
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
    let renderer = MarkdownRenderer::faithful(false, None);
    let result = renderer.render_document(&doc).unwrap();

    assert!(!result.markdown.contains("images/logo.png"));
    assert!(result.markdown.contains("images/figure.png"));
    assert!(result.markdown.contains("*Figure 1: Model overview*"));
}

fn make_flagged_para(text: &str, bold: bool, italic: bool) -> Block {
    let mut block = make_block(0, text, BlockKind::Paragraph, 0);
    block.bold = bold;
    block.italic = italic;
    block
}

fn render_single_block(block: Block) -> String {
    let page = Page {
        page_num: 0,
        width: 595.0,
        height: 842.0,
        blocks: vec![block],
        override_markdown: None,
    };
    let doc = make_doc(vec![page]);
    let renderer = MarkdownRenderer::faithful(false, None);
    renderer.render_document(&doc).unwrap().markdown
}

#[test]
fn bold_paragraph_renders_with_double_asterisks() {
    let md = render_single_block(make_flagged_para("Important note.", true, false));
    assert!(
        md.contains("**Important note.**"),
        "bold paragraph must be wrapped in **…**; got: {md}"
    );
}

#[test]
fn italic_paragraph_renders_with_single_asterisks() {
    let md = render_single_block(make_flagged_para("Side note.", false, true));
    assert!(
        md.contains("*Side note.*") && !md.contains("**"),
        "italic paragraph must be wrapped in *…*; got: {md}"
    );
}

#[test]
fn bold_italic_paragraph_renders_with_triple_asterisks() {
    let md = render_single_block(make_flagged_para("Very important.", true, true));
    assert!(
        md.contains("***Very important.***"),
        "bold+italic must be wrapped in ***…***; got: {md}"
    );
}

#[test]
fn plain_paragraph_unchanged_by_inline_wrap() {
    let md = render_single_block(make_flagged_para("Normal text.", false, false));
    assert!(
        md.contains("Normal text.") && !md.contains('*'),
        "plain paragraph must not gain asterisks; got: {md}"
    );
}

#[test]
fn bold_list_item_renders_with_markers_inside_bullet() {
    let mut block = make_block(
        0,
        "Bold bullet",
        BlockKind::ListItem {
            ordered: false,
            depth: 0,
        },
        0,
    );
    block.bold = true;
    let md = render_single_block(block);
    assert!(
        md.contains("- **Bold bullet**"),
        "bold list item must place markers after the bullet glyph; got: {md}"
    );
}
