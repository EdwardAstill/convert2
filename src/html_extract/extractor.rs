//! HTML extraction: HTML file → Document

use std::path::Path;

use scraper::{Html, Selector};

use crate::document::types::{Block, BlockKind, Document, DocumentMetadata, Page};
use crate::error::{VtvError, VtvResult};

/// Extract an HTML file into a Document.
pub fn extract(path: &Path) -> VtvResult<Document> {
    let html_str = std::fs::read_to_string(path).map_err(|e| VtvError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let document = Html::parse_document(&html_str);
    let metadata = extract_metadata(&document);
    let blocks = extract_blocks(&document);

    let page = Page {
        page_num: 0,
        width: 612.0,
        height: 792.0,
        blocks,
    };

    Ok(Document {
        source_path: path.to_path_buf(),
        pages: vec![page],
        metadata,
    })
}

fn extract_metadata(document: &Html) -> DocumentMetadata {
    let title = Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .map(|el| el.text().collect::<String>().trim().to_string());

    let author = Selector::parse("meta[name='author']")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.to_string());

    DocumentMetadata {
        title,
        author,
        subject: None,
        page_count: 1,
    }
}

fn extract_blocks(document: &Html) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut id = 0;

    // Headings
    for level in 1..=6u8 {
        let selector = Selector::parse(&format!("h{level}")).unwrap();
        for element in document.select(&selector) {
            let text = element.text().collect::<String>().trim().to_string();
            if !text.is_empty() {
                blocks.push(make_block(id, &text, BlockKind::Heading { level }));
                id += 1;
            }
        }
    }

    // Paragraphs
    let p_sel = Selector::parse("p").unwrap();
    for element in document.select(&p_sel) {
        let text = element.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            blocks.push(make_block(id, &text, BlockKind::Paragraph));
            id += 1;
        }
    }

    // List items
    let li_sel = Selector::parse("li").unwrap();
    for element in document.select(&li_sel) {
        let text = element.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            let ordered = element
                .parent()
                .and_then(|p| p.value().as_element())
                .is_some_and(|el| el.name() == "ol");
            blocks.push(make_block(
                id,
                &text,
                BlockKind::ListItem { ordered, depth: 0 },
            ));
            id += 1;
        }
    }

    // Code blocks
    let pre_sel = Selector::parse("pre").unwrap();
    for element in document.select(&pre_sel) {
        let text = element.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            blocks.push(make_block(id, &text, BlockKind::CodeBlock));
            id += 1;
        }
    }

    // Table cells
    let table_sel = Selector::parse("table").unwrap();
    let tr_sel = Selector::parse("tr").unwrap();
    let td_sel = Selector::parse("td, th").unwrap();
    for table in document.select(&table_sel) {
        for (row_idx, tr) in table.select(&tr_sel).enumerate() {
            for (col_idx, cell) in tr.select(&td_sel).enumerate() {
                let text = cell.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    blocks.push(make_block(
                        id,
                        &text,
                        BlockKind::TableCell {
                            row: row_idx,
                            col: col_idx,
                        },
                    ));
                    id += 1;
                }
            }
        }
    }

    blocks
}

fn make_block(id: usize, text: &str, kind: BlockKind) -> Block {
    Block {
        id,
        bbox: crate::document::types::Bbox::new(0.0, 0.0, 612.0, 12.0),
        text: text.to_string(),
        kind,
        font_size: 12.0,
        font_name: "unknown".to_string(),
        page_num: 0,
        reading_order: id,
    }
}
