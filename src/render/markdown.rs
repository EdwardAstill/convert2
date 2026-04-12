use std::path::{Path, PathBuf};
use crate::document::types::{Block, BlockKind, Document, ExtractedImage, Page, Section};
use crate::error::VtvResult;

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

        for page in &doc.pages {
            let (page_md, mut page_images) =
                self.render_page(page, &doc.source_path, &mut image_counter)?;
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
        _source_pdf: &Path,
        _image_counter: &mut usize,
    ) -> VtvResult<(String, Vec<ExtractedImage>)> {
        let mut md = String::new();
        let images: Vec<ExtractedImage> = Vec::new();

        // Sort blocks by reading_order
        let mut blocks: Vec<&Block> = page.blocks.iter().collect();
        blocks.sort_by_key(|b| b.reading_order);

        let mut i = 0;
        while i < blocks.len() {
            let block = blocks[i];
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
                    if let Some(p) = path {
                        md.push_str(&format!("![image]({})\n\n", p));
                    }
                    i += 1;
                }
                // Skip navigation artifacts
                BlockKind::PageNumber
                | BlockKind::RunningHeader
                | BlockKind::RunningFooter => {
                    i += 1;
                }
            }
        }

        Ok((md, images))
    }
}

fn render_heading(text: &str, level: u8) -> String {
    let prefix = "#".repeat(level.clamp(1, 6) as usize);
    format!("{} {}\n\n", prefix, text.trim())
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
    use std::collections::BTreeMap;

    // Build grid: row → col → text
    let mut grid: BTreeMap<usize, BTreeMap<usize, String>> = BTreeMap::new();
    let mut max_col = 0usize;

    for block in blocks {
        if let BlockKind::TableCell { row, col } = block.kind {
            grid.entry(row)
                .or_default()
                .insert(col, block.text.trim().to_string());
            if col > max_col {
                max_col = col;
            }
        }
    }

    if grid.is_empty() {
        return String::new();
    }

    let col_count = max_col + 1;
    let mut result = String::new();
    let mut first_row = true;

    for (_, row_cells) in &grid {
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

/// Split markdown into sections using heading boundaries.
/// Falls back to a single section if no headings are found.
fn split_into_sections(markdown: &str, doc: &Document) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current_title = String::from("Document");
    let mut current_level = 1u8;
    let mut current_content = String::new();
    let mut current_page_start = 1usize;
    let mut found_heading = false;

    for line in markdown.lines() {
        if let Some((level, title)) = parse_heading_line(line) {
            // Flush previous section
            if found_heading || !current_content.trim().is_empty() {
                sections.push(Section {
                    title: current_title.clone(),
                    level: current_level,
                    content: current_content.trim().to_string(),
                    page_start: current_page_start,
                    page_end: current_page_start,
                });
            }
            current_title = title;
            current_level = level;
            current_content = String::new();
            // Approximate — we don't track per-line page numbers
            current_page_start = 1;
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

/// Parse a markdown heading line. Returns `(level, title)` or `None`.
fn parse_heading_line(line: &str) -> Option<(u8, String)> {
    if !line.starts_with('#') {
        return None;
    }
    let trimmed = line.trim_start_matches('#');
    let level = (line.len() - trimmed.len()) as u8;
    if level >= 1 && level <= 6 {
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
        Block {
            id,
            bbox: Bbox::new(0.0, 0.0, 100.0, 20.0),
            text: text.to_string(),
            kind,
            font_size: 12.0,
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
                    BlockKind::ListItem { ordered: false, depth: 0 },
                    0,
                ),
                make_block(
                    1,
                    "Banana",
                    BlockKind::ListItem { ordered: false, depth: 0 },
                    1,
                ),
            ],
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
        };
        let doc = make_doc(vec![page]);
        let renderer = MarkdownRenderer::new(false, None);
        let result = renderer.render_document(&doc).unwrap();
        assert!(result.markdown.contains("| Name | Age |"));
        assert!(result.markdown.contains("| --- |"));
        assert!(result.markdown.contains("| Alice | 30 |"));
    }

    #[test]
    fn parse_heading_line_valid() {
        assert_eq!(
            parse_heading_line("## Hello"),
            Some((2, "Hello".to_string()))
        );
        assert_eq!(
            parse_heading_line("# Top"),
            Some((1, "Top".to_string()))
        );
        assert_eq!(parse_heading_line("Not a heading"), None);
        assert_eq!(parse_heading_line("##"), None); // no title after hashes
    }
}
