//! JSON output format.
//!
//! A structured, machine-readable representation of the document for
//! downstream programmatic consumers — RAG pipelines, LLM tool calls, data
//! pipelines. The schema is a subset of the OpenDataLoader / DoclingDocument
//! item model: one JSON object per document, containing per-page items with
//! bounding boxes and typed content.
//!
//! Emits `<output_dir>/<stem>.json`.

use std::path::Path;

use serde_json::{json, Value};

use crate::document::types::{Block, BlockKind, Document};
use crate::error::{VtvError, VtvResult};
use crate::render::markdown::RenderedDocument;

pub struct JsonFormat;

impl JsonFormat {
    pub fn write(
        _rendered: &RenderedDocument,
        doc: &Document,
        output_dir: &Path,
        stem: &str,
    ) -> VtvResult<()> {
        std::fs::create_dir_all(output_dir).map_err(|e| VtvError::Io {
            path: output_dir.to_path_buf(),
            source: e,
        })?;

        let out_path = output_dir.join(format!("{stem}.json"));
        let payload = build_payload(doc);
        let serialized = serde_json::to_string_pretty(&payload)?;

        std::fs::write(&out_path, serialized).map_err(|e| VtvError::Io {
            path: out_path.clone(),
            source: e,
        })?;
        Ok(())
    }
}

fn build_payload(doc: &Document) -> Value {
    let pages: Vec<Value> = doc
        .pages
        .iter()
        .map(|page| {
            let items: Vec<Value> = page
                .blocks
                .iter()
                .enumerate()
                .map(|(idx, block)| block_to_item(block, page.page_num, idx))
                .collect();
            json!({
                "page_no": page.page_num + 1,
                "width": page.width,
                "height": page.height,
                "items": items,
            })
        })
        .collect();

    json!({
        "source": {
            "path": doc.source_path.display().to_string(),
            "pages": doc.pages.len(),
        },
        "metadata": {
            "title": doc.metadata.title,
            "author": doc.metadata.author,
            "subject": doc.metadata.subject,
            "page_count": doc.metadata.page_count,
        },
        "pages": pages,
    })
}

fn block_to_item(block: &Block, page_num: usize, idx: usize) -> Value {
    let id = format!("p{}-b{}", page_num + 1, idx);
    let bbox = json!([block.bbox.x0, block.bbox.y0, block.bbox.x1, block.bbox.y1]);

    match &block.kind {
        BlockKind::Heading { level } => json!({
            "id": id, "type": "section_header", "level": level,
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::Paragraph => json!({
            "id": id, "type": "paragraph",
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::ListItem { ordered, depth } => json!({
            "id": id, "type": "list_item",
            "ordered": ordered, "depth": depth,
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::TableCell { row, col } => json!({
            "id": id, "type": "table_cell",
            "row": row, "col": col,
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::Caption => json!({
            "id": id, "type": "caption",
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::CodeBlock => json!({
            "id": id, "type": "code_block",
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::PageNumber => json!({
            "id": id, "type": "page_number",
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::RunningHeader => json!({
            "id": id, "type": "running_header",
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::RunningFooter => json!({
            "id": id, "type": "running_footer",
            "text": block.text, "bbox": bbox,
        }),
        BlockKind::Image { path } => json!({
            "id": id, "type": "picture",
            "path": path, "bbox": bbox,
        }),
        BlockKind::Figure { path, caption } => json!({
            "id": id, "type": "figure",
            "path": path, "caption": caption, "bbox": bbox,
        }),
        BlockKind::Formula { latex, display } => json!({
            "id": id, "type": "formula",
            "latex": latex, "display": display, "bbox": bbox,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::types::{Bbox, Block, BlockKind, DocumentMetadata, Page};
    use std::path::PathBuf;

    fn block(kind: BlockKind, text: &str, id: usize) -> Block {
        Block {
            id,
            bbox: Bbox::new(10.0, 20.0, 100.0, 40.0),
            text: text.to_string(),
            kind,
            font_size: 12.0,
            font_name: "Helvetica".to_string(),
            page_num: 0,
            reading_order: id,
        }
    }

    fn doc_with_blocks(blocks: Vec<Block>) -> Document {
        Document {
            source_path: PathBuf::from("/tmp/fake.pdf"),
            pages: vec![Page {
                page_num: 0,
                width: 612.0,
                height: 792.0,
                blocks,
                override_markdown: None,
            }],
            metadata: DocumentMetadata {
                title: Some("Test".to_string()),
                author: None,
                subject: None,
                page_count: 1,
            },
        }
    }

    #[test]
    fn payload_has_top_level_shape() {
        let doc = doc_with_blocks(vec![block(BlockKind::Paragraph, "hi", 0)]);
        let v = build_payload(&doc);
        assert_eq!(v["source"]["pages"], 1);
        assert_eq!(v["metadata"]["title"], "Test");
        assert_eq!(v["pages"].as_array().unwrap().len(), 1);
        assert_eq!(v["pages"][0]["page_no"], 1);
        assert_eq!(v["pages"][0]["width"], 612.0);
    }

    #[test]
    fn heading_item_has_level() {
        let doc = doc_with_blocks(vec![block(
            BlockKind::Heading { level: 2 },
            "Section",
            0,
        )]);
        let v = build_payload(&doc);
        let item = &v["pages"][0]["items"][0];
        assert_eq!(item["type"], "section_header");
        assert_eq!(item["level"], 2);
        assert_eq!(item["text"], "Section");
    }

    #[test]
    fn paragraph_item_round_trips() {
        let doc = doc_with_blocks(vec![block(BlockKind::Paragraph, "hello", 0)]);
        let v = build_payload(&doc);
        let item = &v["pages"][0]["items"][0];
        assert_eq!(item["type"], "paragraph");
        assert_eq!(item["text"], "hello");
        assert_eq!(item["id"], "p1-b0");
        assert_eq!(item["bbox"][0], 10.0);
        assert_eq!(item["bbox"][3], 40.0);
    }

    #[test]
    fn list_item_preserves_ordered_and_depth() {
        let doc = doc_with_blocks(vec![block(
            BlockKind::ListItem { ordered: true, depth: 2 },
            "one",
            0,
        )]);
        let v = build_payload(&doc);
        let item = &v["pages"][0]["items"][0];
        assert_eq!(item["type"], "list_item");
        assert_eq!(item["ordered"], true);
        assert_eq!(item["depth"], 2);
    }

    #[test]
    fn table_cell_preserves_row_col() {
        let doc = doc_with_blocks(vec![block(
            BlockKind::TableCell { row: 3, col: 4 },
            "cell",
            0,
        )]);
        let v = build_payload(&doc);
        let item = &v["pages"][0]["items"][0];
        assert_eq!(item["type"], "table_cell");
        assert_eq!(item["row"], 3);
        assert_eq!(item["col"], 4);
    }

    #[test]
    fn image_item_carries_path() {
        let doc = doc_with_blocks(vec![block(
            BlockKind::Image {
                path: Some("images/page1_img1.png".to_string()),
            },
            "",
            0,
        )]);
        let v = build_payload(&doc);
        let item = &v["pages"][0]["items"][0];
        assert_eq!(item["type"], "picture");
        assert_eq!(item["path"], "images/page1_img1.png");
    }

    #[test]
    fn formula_item_carries_latex_and_display() {
        let doc = doc_with_blocks(vec![block(
            BlockKind::Formula {
                latex: "E = mc^2".to_string(),
                display: true,
            },
            "",
            0,
        )]);
        let v = build_payload(&doc);
        let item = &v["pages"][0]["items"][0];
        assert_eq!(item["type"], "formula");
        assert_eq!(item["latex"], "E = mc^2");
        assert_eq!(item["display"], true);
    }

    #[test]
    fn figure_item_carries_caption() {
        let doc = doc_with_blocks(vec![block(
            BlockKind::Figure {
                path: Some("images/fig1.png".to_string()),
                caption: Some("The transformer.".to_string()),
            },
            "",
            0,
        )]);
        let v = build_payload(&doc);
        let item = &v["pages"][0]["items"][0];
        assert_eq!(item["type"], "figure");
        assert_eq!(item["path"], "images/fig1.png");
        assert_eq!(item["caption"], "The transformer.");
    }

    #[test]
    fn navigation_chrome_is_tagged_not_dropped() {
        // Format writers keep every block; only the markdown renderer strips
        // page numbers, running headers, and running footers.
        let doc = doc_with_blocks(vec![
            block(BlockKind::PageNumber, "1", 0),
            block(BlockKind::RunningHeader, "Chapter 1", 1),
            block(BlockKind::RunningFooter, "footer", 2),
        ]);
        let v = build_payload(&doc);
        let items = v["pages"][0]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["type"], "page_number");
        assert_eq!(items[1]["type"], "running_header");
        assert_eq!(items[2]["type"], "running_footer");
    }
}
