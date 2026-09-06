//! Media stage of the conversion pipeline: figure debug output and image
//! asset persistence.

use std::path::Path;

use anyhow::Context;

use crate::document::types::{Block, BlockKind, ImageRef};
use crate::figure::FigureCandidate;

pub(super) fn write_figure_debug(
    output_dir: &Path,
    page_num: usize,
    candidates: &[FigureCandidate],
) -> anyhow::Result<()> {
    let debug_dir = output_dir.join("debug").join("figures");
    std::fs::create_dir_all(&debug_dir)
        .with_context(|| format!("Failed to create figure debug dir {}", debug_dir.display()))?;
    let path = debug_dir.join(format!("page{}.json", page_num + 1));
    let json = serde_json::to_string_pretty(candidates)?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write figure debug {}", path.display()))
}

pub(super) fn save_page_images(
    image_refs: &[ImageRef],
    images_dir: &Path,
) -> anyhow::Result<Vec<Block>> {
    std::fs::create_dir_all(images_dir)
        .with_context(|| format!("Failed to create images dir {}", images_dir.display()))?;

    let mut blocks: Vec<Block> = Vec::with_capacity(image_refs.len());
    for img_ref in image_refs {
        let filename = format!(
            "page{}_img{}.{}",
            img_ref.page_num + 1,
            img_ref.image_index + 1,
            img_ref.format,
        );
        let abs_path = images_dir.join(&filename);
        std::fs::write(&abs_path, &img_ref.bytes)
            .with_context(|| format!("Failed to write image {}", abs_path.display()))?;
        let rel_path = format!("images/{filename}");
        blocks.push(Block::special(
            1_000_000 + img_ref.image_index,
            img_ref.bbox,
            BlockKind::Image {
                path: Some(rel_path),
            },
            img_ref.page_num,
            0.0,
            "image".to_string(),
        ));
    }
    Ok(blocks)
}
