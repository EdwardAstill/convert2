mod clean;

use self::clean::clean_markdown;

use crate::document::types::{Block, BlockKind, Document, ExtractedImage, Page, Section};
use crate::error::PdfpResult;
use crate::layout::table_inference::detect_structured_region;
use crate::render::media::{
    build_page_media_plan, build_render_context, should_suppress_repeated_text_block, RenderContext,
};
use crate::render::scholarly::{is_abstract_heading, render_scholarly_first_page};
use crate::render::strings::{
    append_paragraph, append_plain_text, escape_comment_attr, inline_wrap,
    normalize_front_matter_text, normalize_heading_text, normalize_paragraph_text,
};
use crate::render::table::{render_coordinate_table, render_structured_region, render_table};
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownStyle {
    /// Keep current PDF-faithful extraction output.
    Faithful,
    /// Produce reader-friendly Markdown by normalizing PDF layout artefacts.
    Clean,
    /// Prefer audit-safe output with conservative extraction choices.
    Review,
}

pub struct MarkdownRenderer {
    /// Whether to extract images to disk. False = skip image extraction.
    pub extract_images: bool,
    /// Directory to write extracted images into (e.g. `output/images/`).
    pub image_output_dir: Option<PathBuf>,
    /// Markdown output style applied after block rendering.
    pub markdown_style: MarkdownStyle,
}

impl MarkdownRenderer {
    pub fn new(extract_images: bool, image_output_dir: Option<PathBuf>) -> Self {
        Self::clean(extract_images, image_output_dir)
    }

    pub fn clean(extract_images: bool, image_output_dir: Option<PathBuf>) -> Self {
        Self::with_style(extract_images, image_output_dir, MarkdownStyle::Clean)
    }

    pub fn faithful(extract_images: bool, image_output_dir: Option<PathBuf>) -> Self {
        Self::with_style(extract_images, image_output_dir, MarkdownStyle::Faithful)
    }

    pub fn with_style(
        extract_images: bool,
        image_output_dir: Option<PathBuf>,
        markdown_style: MarkdownStyle,
    ) -> Self {
        Self {
            extract_images,
            image_output_dir,
            markdown_style,
        }
    }

    pub fn render_document(&self, doc: &Document) -> PdfpResult<RenderedDocument> {
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

        if matches!(self.markdown_style, MarkdownStyle::Clean) {
            all_markdown = clean_markdown(&all_markdown);
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
    ) -> PdfpResult<(String, Vec<ExtractedImage>)> {
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
        if let Some(front_matter) =
            render_scholarly_first_page(&blocks, page, &media_plan, append_rendered_block)
        {
            md.push_str(&front_matter.markdown);
            i = front_matter.next_index;
        }

        while i < blocks.len() {
            if let Some(structured) = detect_structured_region(&blocks, i) {
                render_structured_region(&mut md, &blocks, i, &structured);
                i = structured.next_index;
                continue;
            }

            let block = blocks[i];
            if let Some(override_md) = &block.override_markdown {
                md.push_str(override_md.trim_end());
                md.push_str("\n\n");
                i += 1;
                continue;
            }
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
                    let text = normalize_paragraph_text(&block.text);
                    if !text.is_empty() {
                        md.push_str(&inline_wrap(&text, block.bold, block.italic));
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
                BlockKind::CoordinateTable { table } => {
                    md.push_str(&render_coordinate_table(table));
                    md.push('\n');
                    i += 1;
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
                BlockKind::FormulaReview { reason, crop_path } => {
                    md.push_str(&format!(
                        "<!-- formula-review: page={} reason=\"{}\"",
                        page.page_num + 1,
                        escape_comment_attr(reason)
                    ));
                    if let Some(path) = crop_path {
                        md.push_str(&format!(" crop=\"{}\"", escape_comment_attr(path)));
                    }
                    md.push_str(" -->\n\n");
                    i += 1;
                }
                // Skip navigation artifacts
                BlockKind::PageNumber
                | BlockKind::RunningHeader
                | BlockKind::RunningFooter
                | BlockKind::Artifact => {
                    i += 1;
                }
            }
        }

        Ok((md, images))
    }
}

fn render_heading(text: &str, level: u8) -> String {
    let prefix = "#".repeat(level.clamp(1, 6) as usize);
    format!("{} {}\n\n", prefix, normalize_heading_text(text.trim()))
}

fn render_list(blocks: &[&Block]) -> String {
    let mut result = String::new();
    let mut ordered_counters: std::collections::HashMap<u8, usize> =
        std::collections::HashMap::new();

    for block in blocks {
        if let BlockKind::ListItem { ordered, depth } = &block.kind {
            let indent = "  ".repeat(*depth as usize);
            let item_text = inline_wrap(block.text.trim(), block.bold, block.italic);
            if *ordered {
                let counter = ordered_counters.entry(*depth).or_insert(0);
                *counter += 1;
                result.push_str(&format!("{}{}. {}\n", indent, counter, item_text));
            } else {
                result.push_str(&format!("{}- {}\n", indent, item_text));
            }
        }
    }
    result
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
        BlockKind::FormulaReview { reason, crop_path } => {
            markdown.push_str(&format!(
                "<!-- formula-review: reason=\"{}\"",
                escape_comment_attr(reason)
            ));
            if let Some(path) = crop_path {
                markdown.push_str(&format!(" crop=\"{}\"", escape_comment_attr(path)));
            }
            markdown.push_str(" -->\n\n");
        }
        BlockKind::CoordinateTable { table } if !force_plain_text => {
            markdown.push_str(&render_coordinate_table(table));
            markdown.push('\n');
        }
        BlockKind::ListItem { ordered, depth } => {
            let indent = "  ".repeat(*depth as usize);
            let item_text = inline_wrap(block.text.trim(), block.bold, block.italic);
            if *ordered {
                markdown.push_str(&format!("{}1. {}\n\n", indent, item_text));
            } else {
                markdown.push_str(&format!("{}- {}\n\n", indent, item_text));
            }
        }
        BlockKind::PageNumber
        | BlockKind::RunningHeader
        | BlockKind::RunningFooter
        | BlockKind::Artifact => {}
        _ if force_plain_text => {
            let text = normalize_front_matter_text(block.text.trim());
            append_plain_text(markdown, &text);
        }
        _ => append_paragraph(
            markdown,
            &inline_wrap(
                &normalize_front_matter_text(block.text.trim()),
                block.bold,
                block.italic,
            ),
        ),
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
mod tests;
