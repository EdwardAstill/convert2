//! EPUB extraction: ZIP(XHTML chapters) → Document

use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;
use scraper::{Html, Selector};

use crate::document::types::{Block, BlockKind, Document, DocumentMetadata, Page};
use crate::error::{VtvError, VtvResult};

/// Extract an EPUB file into a Document.
pub fn extract(path: &Path) -> VtvResult<Document> {
    let file = std::fs::File::open(path).map_err(|e| VtvError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| VtvError::EpubParse {
        path: path.to_path_buf(),
        message: format!("Failed to open ZIP: {e}"),
    })?;

    // Find the .opf file via META-INF/container.xml
    let opf_path = find_opf_path(&mut archive, path)?;

    // Parse the .opf to get metadata and spine (reading order)
    let opf_xml = read_zip_entry_str(&mut archive, &opf_path, path)?;
    let (metadata, spine_hrefs) = parse_opf(&opf_xml, &opf_path);

    // Determine base directory of the OPF file
    let opf_dir = opf_path
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default();

    // Parse each chapter in spine order
    let mut pages = Vec::new();
    let mut global_block_id = 0;

    for (chapter_idx, href) in spine_hrefs.iter().enumerate() {
        let full_path = format!("{opf_dir}{href}");
        let html = match read_zip_entry_str(&mut archive, &full_path, path) {
            Ok(h) => h,
            Err(_) => continue, // skip missing chapters
        };

        let blocks = parse_html_to_blocks(&html, &mut global_block_id, chapter_idx);
        if !blocks.is_empty() {
            pages.push(Page {
                page_num: chapter_idx,
                width: 612.0,
                height: 792.0,
                blocks,
            });
        }
    }

    Ok(Document {
        source_path: path.to_path_buf(),
        pages,
        metadata,
    })
}

fn read_zip_entry_str(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    path: &Path,
) -> VtvResult<String> {
    let mut entry = archive.by_name(name).map_err(|e| VtvError::EpubParse {
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

fn find_opf_path(
    archive: &mut zip::ZipArchive<std::fs::File>,
    path: &Path,
) -> VtvResult<String> {
    let container_xml = read_zip_entry_str(archive, "META-INF/container.xml", path)?;
    let mut reader = Reader::from_str(&container_xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.local_name().as_ref() == b"rootfile" {
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"full-path" {
                            return Ok(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Err(VtvError::EpubParse {
        path: path.to_path_buf(),
        message: "Could not find rootfile in container.xml".to_string(),
    })
}

fn parse_opf(opf_xml: &str, _opf_path: &str) -> (DocumentMetadata, Vec<String>) {
    let mut reader = Reader::from_str(opf_xml);
    let mut buf = Vec::new();

    let mut title = None;
    let mut author = None;
    let mut current_tag = String::new();

    // manifest: id → href
    let mut manifest = std::collections::HashMap::new();
    // spine: ordered list of idref
    let mut spine_idrefs = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "title" | "creator" => current_tag = local,
                    "item" => {
                        let mut id = String::new();
                        let mut href = String::new();
                        for attr in e.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
                                b"href" => {
                                    href = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }
                        if !id.is_empty() && !href.is_empty() {
                            manifest.insert(id, href);
                        }
                    }
                    "itemref" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"idref" {
                                spine_idrefs
                                    .push(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.decode().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "title" => title = Some(text),
                    "creator" => author = Some(text),
                    _ => {}
                }
            }
            Ok(Event::End(_)) => current_tag.clear(),
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    let spine_hrefs: Vec<String> = spine_idrefs
        .iter()
        .filter_map(|idref| manifest.get(idref).cloned())
        .collect();

    let metadata = DocumentMetadata {
        title,
        author,
        subject: None,
        page_count: spine_hrefs.len(),
    };

    (metadata, spine_hrefs)
}

fn parse_html_to_blocks(html: &str, block_id: &mut usize, page_num: usize) -> Vec<Block> {
    let document = Html::parse_document(html);
    let mut blocks = Vec::new();

    // Process headings
    for level in 1..=6u8 {
        let selector = Selector::parse(&format!("h{level}")).unwrap();
        for element in document.select(&selector) {
            let text = element.text().collect::<String>().trim().to_string();
            if !text.is_empty() {
                blocks.push(make_block(
                    *block_id,
                    &text,
                    BlockKind::Heading { level },
                    page_num,
                ));
                *block_id += 1;
            }
        }
    }

    // Process paragraphs
    let p_sel = Selector::parse("p").unwrap();
    for element in document.select(&p_sel) {
        let text = element.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            blocks.push(make_block(*block_id, &text, BlockKind::Paragraph, page_num));
            *block_id += 1;
        }
    }

    // Process list items
    let li_sel = Selector::parse("li").unwrap();
    for element in document.select(&li_sel) {
        let text = element.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            // Determine if ordered by checking parent
            let ordered = element
                .parent()
                .and_then(|p| p.value().as_element())
                .is_some_and(|el| el.name() == "ol");
            blocks.push(make_block(
                *block_id,
                &text,
                BlockKind::ListItem { ordered, depth: 0 },
                page_num,
            ));
            *block_id += 1;
        }
    }

    // Process code blocks
    let pre_sel = Selector::parse("pre").unwrap();
    for element in document.select(&pre_sel) {
        let text = element.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            blocks.push(make_block(*block_id, &text, BlockKind::CodeBlock, page_num));
            *block_id += 1;
        }
    }

    blocks
}

fn make_block(id: usize, text: &str, kind: BlockKind, page_num: usize) -> Block {
    Block {
        id,
        bbox: crate::document::types::Bbox::new(0.0, 0.0, 612.0, 12.0),
        text: text.to_string(),
        kind,
        font_size: 12.0,
        font_name: "unknown".to_string(),
        page_num,
        reading_order: id,
    }
}
