//! SVG → PNG conversion using resvg.

use std::path::Path;

use crate::error::{VtvError, VtvResult};

/// Convert an SVG file to PNG.
pub fn convert(svg_path: &Path, output_path: &Path) -> VtvResult<()> {
    let svg_data = std::fs::read(svg_path).map_err(|e| VtvError::Io {
        path: svg_path.to_path_buf(),
        source: e,
    })?;

    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&svg_data, &options).map_err(|e| {
        VtvError::SvgConvert {
            path: svg_path.to_path_buf(),
            message: format!("Failed to parse SVG: {e}"),
        }
    })?;

    let size = tree.size();
    let width = size.width().ceil() as u32;
    let height = size.height().ceil() as u32;

    if width == 0 || height == 0 {
        return Err(VtvError::SvgConvert {
            path: svg_path.to_path_buf(),
            message: "SVG has zero dimensions".to_string(),
        });
    }

    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| VtvError::SvgConvert {
            path: svg_path.to_path_buf(),
            message: "Failed to create pixel buffer".to_string(),
        })?;

    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());

    let png_data = pixmap.encode_png().map_err(|e| VtvError::SvgConvert {
        path: svg_path.to_path_buf(),
        message: format!("Failed to encode PNG: {e}"),
    })?;

    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VtvError::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    std::fs::write(output_path, png_data).map_err(|e| VtvError::Io {
        path: output_path.to_path_buf(),
        source: e,
    })?;

    Ok(())
}
