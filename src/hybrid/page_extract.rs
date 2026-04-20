//! Extract a single page of a PDF as a standalone single-page PDF.
//!
//! Used by the hybrid backend to upload only the pages that triage flagged
//! for Docling routing, keeping network traffic proportional to the number
//! of complex pages rather than the whole document.
//!
//! Implementation: open the PDF as a `PdfDocument`, `delete_page` every page
//! except the target (in reverse order so indices stay valid), save to a
//! temp file, read the bytes back, delete the temp file. mupdf handles the
//! heavy lifting of garbage-collecting orphaned objects on save.

use std::path::{Path, PathBuf};

use mupdf::pdf::PdfDocument;

use crate::error::{VtvError, VtvResult};

/// Extract page `target_page` (0-indexed) of the PDF at `src` as a
/// single-page PDF and return the encoded bytes.
pub fn extract_page_as_pdf_bytes(src: &Path, target_page: usize) -> VtvResult<Vec<u8>> {
    let src_str = src.to_string_lossy();
    let mut pdf = PdfDocument::open(src_str.as_ref()).map_err(|e| VtvError::PdfOpen {
        path: src.to_path_buf(),
        message: e.to_string(),
    })?;

    let page_count = pdf.page_count().map_err(|e| VtvError::PdfExtraction {
        page: target_page,
        message: e.to_string(),
    })?;

    if target_page as i32 >= page_count {
        return Err(VtvError::PdfExtraction {
            page: target_page,
            message: format!(
                "target page {} out of range (doc has {} pages)",
                target_page, page_count
            ),
        });
    }

    // Delete every other page, highest index first so indices remain stable.
    for i in (0..page_count).rev() {
        if i == target_page as i32 {
            continue;
        }
        pdf.delete_page(i).map_err(|e| VtvError::PdfExtraction {
            page: i as usize,
            message: format!("delete_page failed: {e}"),
        })?;
    }

    let temp_path = temp_file_path(src, target_page);
    let temp_str = temp_path.to_string_lossy();
    pdf.save(temp_str.as_ref()).map_err(|e| VtvError::Io {
        path: temp_path.clone(),
        source: std::io::Error::other(e.to_string()),
    })?;

    let bytes = std::fs::read(&temp_path).map_err(|e| VtvError::Io {
        path: temp_path.clone(),
        source: e,
    })?;
    let _ = std::fs::remove_file(&temp_path);

    Ok(bytes)
}

fn temp_file_path(src: &Path, target_page: usize) -> PathBuf {
    let mut name = std::env::temp_dir();
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_string());
    name.push(format!(
        "cnv-{pid}-{stem}-p{page}.pdf",
        pid = std::process::id(),
        page = target_page,
    ));
    name
}
