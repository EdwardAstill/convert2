use std::collections::HashMap;
use std::path::Path;

use mupdf::{Document, MetadataName, TextPageFlags};
use mupdf::text_page::TextBlockType;

use crate::document::types::{Bbox, DocumentMetadata, ImageRef, RawPage, RawTextBlock};
use crate::error::{VtvError, VtvResult};

pub struct PdfExtractor;

impl PdfExtractor {
    /// Extract all pages from a PDF file.
    pub fn extract_pages(path: &Path) -> VtvResult<Vec<RawPage>> {
        let path_str = path.to_string_lossy();
        let doc = Document::open(path_str.as_ref()).map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let page_count = doc.page_count().map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let mut pages = Vec::with_capacity(page_count as usize);
        for i in 0..page_count {
            let page = Self::extract_page(&doc, i as usize)?;
            pages.push(page);
        }
        Ok(pages)
    }

    /// Extract document metadata (title, author, subject, page count).
    pub fn extract_metadata(path: &Path) -> VtvResult<DocumentMetadata> {
        let path_str = path.to_string_lossy();
        let doc = Document::open(path_str.as_ref()).map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let page_count = doc.page_count().map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })? as usize;

        let title = doc
            .metadata(MetadataName::Title)
            .ok()
            .and_then(|s| if s.is_empty() { None } else { Some(s) });

        let author = doc
            .metadata(MetadataName::Author)
            .ok()
            .and_then(|s| if s.is_empty() { None } else { Some(s) });

        let subject = doc
            .metadata(MetadataName::Subject)
            .ok()
            .and_then(|s| if s.is_empty() { None } else { Some(s) });

        Ok(DocumentMetadata {
            title,
            author,
            subject,
            page_count,
        })
    }

    // --- private helpers ---

    fn extract_page(doc: &Document, page_num: usize) -> VtvResult<RawPage> {
        let page = doc.load_page(page_num as i32).map_err(|e| VtvError::PdfExtraction {
            page: page_num,
            message: e.to_string(),
        })?;

        let bounds = page.bounds().map_err(|e| VtvError::PdfExtraction {
            page: page_num,
            message: e.to_string(),
        })?;

        let text_page = page
            .to_text_page(TextPageFlags::PRESERVE_IMAGES)
            .map_err(|e| VtvError::PdfExtraction {
                page: page_num,
                message: e.to_string(),
            })?;

        let mut blocks: Vec<RawTextBlock> = Vec::new();
        let mut image_refs: Vec<ImageRef> = Vec::new();
        let mut image_index: usize = 0;

        for (block_id, block) in text_page.blocks().enumerate() {
            match block.r#type() {
                TextBlockType::Text => {
                    let text = Self::collect_block_text(&block);
                    if text.is_empty() {
                        continue;
                    }
                    let font_size = Self::dominant_font_size(&block);
                    let font_name = Self::dominant_font_name(&block);
                    let bbox = Self::mupdf_rect_to_bbox(block.bounds());

                    blocks.push(RawTextBlock {
                        bbox,
                        text,
                        font_size,
                        font_name,
                        page_num,
                        block_id,
                        reading_order: 0,
                    });
                }
                TextBlockType::Image => {
                    let bbox = Self::mupdf_rect_to_bbox(block.bounds());
                    image_refs.push(ImageRef {
                        page_num,
                        bbox,
                        image_index,
                    });
                    image_index += 1;
                }
                // Ignore Struct, Vector, Grid block types
                _ => {}
            }
        }

        Ok(RawPage {
            page_num,
            width: bounds.x1 - bounds.x0,
            height: bounds.y1 - bounds.y0,
            blocks,
            image_refs,
        })
    }

    fn mupdf_rect_to_bbox(r: mupdf::Rect) -> Bbox {
        Bbox::new(r.x0, r.y0, r.x1, r.y1)
    }

    /// Collect all text from a TextBlock's lines into a single string.
    fn collect_block_text(block: &mupdf::TextBlock<'_>) -> String {
        let mut lines_text: Vec<String> = Vec::new();

        for line in block.lines() {
            let line_str: String = line
                .chars()
                .filter_map(|ch| ch.char())
                .collect();
            let trimmed = line_str.trim_end().to_owned();
            if !trimmed.is_empty() {
                lines_text.push(trimmed);
            }
        }

        lines_text.join("\n")
    }

    /// Get the dominant font size in a TextBlock (mode of char sizes).
    fn dominant_font_size(block: &mupdf::TextBlock<'_>) -> f32 {
        // Collect sizes bucketed to 1 decimal place to handle floating point variation
        let mut counts: HashMap<u32, (usize, f32)> = HashMap::new();

        for line in block.lines() {
            for ch in line.chars() {
                let size = ch.size();
                // Key by rounded-to-nearest-tenth: multiply by 10, round, cast to u32
                let key = (size * 10.0).round() as u32;
                let entry = counts.entry(key).or_insert((0, size));
                entry.0 += 1;
            }
        }

        if counts.is_empty() {
            return 12.0;
        }

        counts
            .into_values()
            .max_by_key(|(count, _)| *count)
            .map(|(_, size)| size)
            .unwrap_or(12.0)
    }

    /// Get the dominant font name in a TextBlock.
    /// Note: the mupdf 0.6.0 Rust wrapper does not expose per-char font name,
    /// so this always returns "unknown".
    fn dominant_font_name(_block: &mupdf::TextBlock<'_>) -> String {
        // The mupdf::TextChar API only exposes char(), origin(), size(), quad().
        // Font name requires direct fz_stext_char.style->font access which is not
        // surfaced by the wrapper. Return "unknown" as specified fallback.
        "unknown".to_owned()
    }
}
