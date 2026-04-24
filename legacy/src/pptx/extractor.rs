//! PPTX extraction: ZIP(OOXML slides) → Document

use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::document::types::{Block, BlockKind, Document, DocumentMetadata, Page};
use crate::error::{VtvError, VtvResult};

/// Extract a PPTX file into a Document.
pub fn extract(path: &Path) -> VtvResult<Document> {
    let file = std::fs::File::open(path).map_err(|e| VtvError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| VtvError::PptxParse {
        path: path.to_path_buf(),
        message: format!("Failed to open ZIP: {e}"),
    })?;

    // Find all slide files
    let slide_names: Vec<String> = (1..)
        .map(|i| format!("ppt/slides/slide{i}.xml"))
        .take_while(|name| archive.by_name(name).is_ok())
        .collect();

    let metadata = parse_metadata(&mut archive);

    let mut pages = Vec::new();
    let mut global_block_id = 0;

    for (idx, slide_name) in slide_names.iter().enumerate() {
        let xml = read_zip_entry(&mut archive, slide_name, path)?;
        let blocks = parse_slide_xml(&xml, &mut global_block_id, idx);

        pages.push(Page {
            page_num: idx,
            width: 960.0,  // standard 10" slide at 96dpi
            height: 540.0, // 16:9 aspect
            blocks,
            override_markdown: None,
        });
    }

    // Parse speaker notes if available
    for (idx, _) in slide_names.iter().enumerate() {
        let notes_name = format!("ppt/notesSlides/notesSlide{}.xml", idx + 1);
        if let Ok(xml) = read_zip_entry(&mut archive, &notes_name, path) {
            let note_text = extract_text_from_xml(&xml);
            if !note_text.is_empty() {
                if let Some(page) = pages.get_mut(idx) {
                    page.blocks.push(Block {
                        id: global_block_id,
                        bbox: crate::document::types::Bbox::new(0.0, 0.0, 960.0, 12.0),
                        text: format!("[Speaker Notes] {note_text}"),
                        kind: BlockKind::Paragraph,
                        font_size: 10.0,
                        font_name: "unknown".to_string(),
                        page_num: idx,
                        reading_order: global_block_id,
                    });
                    global_block_id += 1;
                }
            }
        }
    }

    Ok(Document {
        source_path: path.to_path_buf(),
        pages,
        metadata,
    })
}

fn read_zip_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    path: &Path,
) -> VtvResult<String> {
    let mut entry = archive.by_name(name).map_err(|e| VtvError::PptxParse {
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
    let mut current_tag = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                current_tag =
                    String::from_utf8_lossy(e.local_name().as_ref()).to_string();
            }
            Ok(Event::Text(ref e)) => {
                let text = e.decode().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "title" => title = Some(text),
                    "creator" => author = Some(text),
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
        subject: None,
        page_count: 0, // will be set by page count
    }
}

fn parse_slide_xml(xml: &str, block_id: &mut usize, page_num: usize) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    let mut in_shape = false;
    let mut shape_texts: Vec<String> = Vec::new();
    let mut current_para_text = String::new();
    let mut is_title = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "sp" => {
                        in_shape = true;
                        shape_texts.clear();
                        is_title = false;
                    }
                    "ph" if in_shape => {
                        // Placeholder type — detect title shapes
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"type" {
                                let val = String::from_utf8_lossy(&attr.value);
                                if val == "title" || val == "ctrTitle" {
                                    is_title = true;
                                }
                            }
                        }
                    }
                    "p" if in_shape => {
                        current_para_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_shape => {
                let text = e.decode().unwrap_or_default().to_string();
                current_para_text.push_str(&text);
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "p" if in_shape => {
                        let text = current_para_text.trim().to_string();
                        if !text.is_empty() {
                            shape_texts.push(text);
                        }
                    }
                    "sp" if in_shape => {
                        in_shape = false;
                        for text in &shape_texts {
                            let kind = if is_title {
                                BlockKind::Heading { level: 1 }
                            } else {
                                BlockKind::Paragraph
                            };
                            blocks.push(Block {
                                id: *block_id,
                                bbox: crate::document::types::Bbox::new(0.0, 0.0, 960.0, 12.0),
                                text: text.clone(),
                                kind,
                                font_size: if is_title { 24.0 } else { 12.0 },
                                font_name: "unknown".to_string(),
                                page_num,
                                reading_order: *block_id,
                            });
                            *block_id += 1;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    blocks
}

fn extract_text_from_xml(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut text_parts = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(ref e)) => {
                let text = e.decode().unwrap_or_default().to_string();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    text_parts.push(trimmed.to_string());
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    text_parts.join(" ")
}
