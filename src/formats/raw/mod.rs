use crate::document::types::Document;
use crate::error::{VtvError, VtvResult};
use crate::render::markdown::RenderedDocument;
use std::fs;
use std::path::Path;

pub struct RawFormat;

impl RawFormat {
    pub fn write(
        rendered: &RenderedDocument,
        _doc: &Document,
        output_dir: &Path,
        stem: &str,
    ) -> VtvResult<()> {
        fs::create_dir_all(output_dir).map_err(|e| VtvError::Io {
            path: output_dir.to_path_buf(),
            source: e,
        })?;

        // Write markdown file
        let md_path = output_dir.join(format!("{}.md", stem));
        fs::write(&md_path, &rendered.markdown).map_err(|e| VtvError::Io {
            path: md_path.clone(),
            source: e,
        })?;

        // Copy extracted images
        if !rendered.images.is_empty() {
            let img_dir = output_dir.join("images");
            fs::create_dir_all(&img_dir).map_err(|e| VtvError::Io {
                path: img_dir.clone(),
                source: e,
            })?;
            for img in &rendered.images {
                let dest = img_dir.join(&img.rel_path);
                fs::copy(&img.abs_path, &dest).map_err(|e| VtvError::Io {
                    path: dest.clone(),
                    source: e,
                })?;
            }
        }

        println!("  wrote {}", md_path.display());
        Ok(())
    }
}
