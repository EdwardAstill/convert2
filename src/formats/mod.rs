//! Output format writers.
//!
//! Currently a single raw writer: one `<stem>.md` file per document, with
//! page markers stripped and extracted images copied alongside.

use crate::document::types::Document;
use crate::error::{PdfpError, PdfpResult};
use crate::render::markdown::RenderedDocument;
use std::fs;
use std::path::Path;

pub struct RawFormat;

/// Strip page boundary markers from rendered markdown.
fn strip_page_markers(markdown: &str) -> String {
    markdown
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("<!-- page:") && trimmed.ends_with(" -->"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl RawFormat {
    pub fn write(
        rendered: &RenderedDocument,
        _doc: &Document,
        output_dir: &Path,
        stem: &str,
    ) -> PdfpResult<()> {
        fs::create_dir_all(output_dir).map_err(|e| PdfpError::Io {
            path: output_dir.to_path_buf(),
            source: e,
        })?;

        // Write markdown file with page markers stripped
        let md_path = output_dir.join(format!("{}.md", stem));
        let clean = strip_page_markers(&rendered.markdown);
        fs::write(&md_path, &clean).map_err(|e| PdfpError::Io {
            path: md_path.clone(),
            source: e,
        })?;

        // Copy extracted images
        if !rendered.images.is_empty() {
            let img_dir = output_dir.join("images");
            fs::create_dir_all(&img_dir).map_err(|e| PdfpError::Io {
                path: img_dir.clone(),
                source: e,
            })?;
            for img in &rendered.images {
                let dest = img_dir.join(&img.rel_path);
                fs::copy(&img.abs_path, &dest).map_err(|e| PdfpError::Io {
                    path: dest.clone(),
                    source: e,
                })?;
            }
        }

        println!("  wrote {}", md_path.display());
        Ok(())
    }
}
