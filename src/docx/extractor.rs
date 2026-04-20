//! DOCX extraction: ZIP(OOXML) → Document

use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::document::types::{Block, BlockKind, Document, DocumentMetadata, Page};
use crate::error::{VtvError, VtvResult};

/// Extract a DOCX file into a Document.
pub fn extract(path: &Path) -> VtvResult<Document> {
    let file = std::fs::File::open(path).map_err(|e| VtvError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| VtvError::DocxParse {
        path: path.to_path_buf(),
        message: format!("Failed to open ZIP: {e}"),
    })?;

    // Read word/document.xml
    let xml = read_zip_entry(&mut archive, "word/document.xml", path)?;

    // Parse core.xml for metadata
    let metadata = parse_metadata(&mut archive);

    // Parse document.xml into blocks
    let blocks = parse_document_xml(&xml, path)?;

    let page = Page {
        page_num: 0,
        width: 612.0, // standard letter size in points
        height: 792.0,
        blocks,
        override_markdown: None,
    };

    Ok(Document {
        source_path: path.to_path_buf(),
        pages: vec![page],
        metadata,
    })
}

fn read_zip_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    path: &Path,
) -> VtvResult<String> {
    let mut entry = archive.by_name(name).map_err(|e| VtvError::DocxParse {
        path: path.to_path_buf(),
        message: format!("Missing {name}: {e}"),
    })?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).map_err(|e| VtvError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(buf)
}

fn parse_metadata(archive: &mut zip::ZipArchive<std::fs::File>) -> DocumentMetadata {
    let Ok(mut entry) = archive.by_name("docProps/core.xml") else {
        return DocumentMetadata::default();
    };
    let mut xml = String::new();
    if entry.read_to_string(&mut xml).is_err() {
        return DocumentMetadata::default();
    }

    let mut reader = Reader::from_str(&xml);
    let mut title = None;
    let mut author = None;
    let mut subject = None;
    let mut current_tag = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                current_tag = name;
            }
            Ok(Event::Text(ref e)) => {
                let text = e.decode().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "title" => title = Some(text),
                    "creator" => author = Some(text),
                    "subject" => subject = Some(text),
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    DocumentMetadata {
        title,
        author,
        subject,
        page_count: 1,
    }
}

fn parse_document_xml(xml: &str, path: &Path) -> VtvResult<Vec<Block>> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut blocks = Vec::new();
    let mut block_id = 0;

    // State
    let mut in_paragraph = false;
    let mut paragraph_text = String::new();
    let mut paragraph_style = String::new();
    let mut _in_table = false;
    let mut table_row: usize = 0;
    let mut table_col: usize = 0;
    let mut cell_text = String::new();
    let mut in_cell = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "p" => {
                        in_paragraph = true;
                        paragraph_text.clear();
                        paragraph_style.clear();
                    }
                    "pStyle" => {
                        // Extract val attribute for heading detection
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                paragraph_style =
                                    String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                    }
                    "tbl" => {
                        _in_table = true;
                        table_row = 0;
                    }
                    "tr" => {
                        table_col = 0;
                    }
                    "tc" => {
                        in_cell = true;
                        cell_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.decode().unwrap_or_default().to_string();
                if in_cell {
                    cell_text.push_str(&text);
                } else if in_paragraph {
                    paragraph_text.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "p" if in_paragraph => {
                        in_paragraph = false;
                        let text = paragraph_text.trim().to_string();
                        if !text.is_empty() && !in_cell {
                            let kind = classify_paragraph_style(&paragraph_style);
                            blocks.push(make_block(block_id, &text, kind));
                            block_id += 1;
                        }
                        if in_cell {
                            // cell text already captured
                        }
                    }
                    "tc" if in_cell => {
                        in_cell = false;
                        let text = cell_text.trim().to_string();
                        if !text.is_empty() {
                            blocks.push(make_block(
                                block_id,
                                &text,
                                BlockKind::TableCell {
                                    row: table_row,
                                    col: table_col,
                                },
                            ));
                            block_id += 1;
                        }
                        table_col += 1;
                    }
                    "tr" => {
                        table_row += 1;
                    }
                    "tbl" => {
                        _in_table = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(VtvError::DocxParse {
                    path: path.to_path_buf(),
                    message: format!("XML parse error: {e}"),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(blocks)
}

fn classify_paragraph_style(style: &str) -> BlockKind {
    let lower = style.to_lowercase();
    if lower.starts_with("heading") || lower.starts_with("title") {
        // Extract heading level from style name like "Heading1", "Heading2"
        let level = lower
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u8>()
            .unwrap_or(1)
            .clamp(1, 6);
        BlockKind::Heading { level }
    } else if lower.contains("listparagraph") || lower.contains("listbullet") {
        BlockKind::ListItem {
            ordered: lower.contains("number"),
            depth: 0,
        }
    } else if lower.contains("caption") {
        BlockKind::Caption
    } else {
        BlockKind::Paragraph
    }
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
