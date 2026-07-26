use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use lopdf::{Document as LopdfDocument, Object};
use mupdf::{Document, DocumentWriter, Matrix, Rect};

use crate::cli::{CropArgs, PageCommand, PageSubcommand, ResizeArgs};
use crate::processor::page_range::parse_page_selection;

pub fn run(args: &PageCommand) -> anyhow::Result<()> {
    match &args.command {
        PageSubcommand::Resize(args) => resize(args),
        PageSubcommand::Crop(args) => crop(args),
        PageSubcommand::Text(args) => crate::processor::text::run(args),
    }
}

fn resize(args: &ResizeArgs) -> anyhow::Result<()> {
    let (target_width, target_height) = paper_size(&args.paper)?;
    let fit = FitMode::parse(&args.fit)?;
    let media_box = Rect::new(0.0, 0.0, target_width, target_height);

    let input_str = args.input.to_string_lossy();
    let doc = Document::open(input_str.as_ref())
        .with_context(|| format!("failed to open {}", args.input.display()))?;
    if let Some(parent) = args
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let output_str = args.output.to_string_lossy();
    let mut writer = DocumentWriter::new(output_str.as_ref(), "pdf", "")
        .with_context(|| format!("failed to create {}", args.output.display()))?;

    for page_idx in 0..doc.page_count()? {
        let page = doc.load_page(page_idx)?;
        let bounds = page.bounds()?;
        let (sx, sy) = match fit {
            FitMode::Stretch => (
                target_width / bounds.width(),
                target_height / bounds.height(),
            ),
            FitMode::Contain => {
                let scale = (target_width / bounds.width()).min(target_height / bounds.height());
                (scale, scale)
            }
            FitMode::Cover => {
                let scale = (target_width / bounds.width()).max(target_height / bounds.height());
                (scale, scale)
            }
        };
        let rendered_width = bounds.width() * sx;
        let rendered_height = bounds.height() * sy;
        let x = (target_width - rendered_width) / 2.0;
        let y = (target_height - rendered_height) / 2.0;
        let matrix = Matrix::new(sx, 0.0, 0.0, sy, x - bounds.x0 * sx, y - bounds.y0 * sy);

        let device = writer.begin_page(media_box)?;
        page.run(&device, &matrix)
            .with_context(|| format!("failed to render page {}", page_idx + 1))?;
        writer.end_page(device)?;
    }

    Ok(())
}

fn crop(args: &CropArgs) -> anyhow::Result<()> {
    ensure_output_is_not_input(&args.input, &args.output)?;
    let [x0, y0, x1, y1]: [f32; 4] = args
        .crop_box
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("--box requires exactly four values: x0 y0 x1 y1"))?;
    if x1 <= x0 || y1 <= y0 {
        bail!("--box must satisfy x1 > x0 and y1 > y0");
    }

    let page_count = page_count(&args.input)?;
    let selected: BTreeSet<usize> = parse_page_selection(&args.pages, page_count)?
        .into_iter()
        .collect();
    let mut doc = LopdfDocument::load(&args.input)
        .with_context(|| format!("failed to open {}", args.input.display()))?;
    let pages = doc.get_pages();
    let crop_box = vec![x0.into(), y0.into(), x1.into(), y1.into()];

    for page_index in selected {
        let page_num = (page_index + 1) as u32;
        let page_id = pages
            .get(&page_num)
            .copied()
            .with_context(|| format!("page {page_num} not found in {}", args.input.display()))?;
        let page = doc
            .get_object_mut(page_id)
            .with_context(|| format!("failed to access page {page_num}"))?
            .as_dict_mut()?;
        page.set("CropBox", Object::Array(crop_box.clone()));
    }

    save_lopdf(&mut doc, &args.output)?;
    eprintln!("wrote {}", args.output.display());
    Ok(())
}

fn page_count(path: &Path) -> anyhow::Result<usize> {
    let doc =
        LopdfDocument::load(path).with_context(|| format!("failed to open {}", path.display()))?;
    Ok(doc.get_pages().len())
}

fn save_lopdf(doc: &mut LopdfDocument, output: &Path) -> anyhow::Result<()> {
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    doc.save(output)
        .with_context(|| format!("failed to save {}", output.display()))?;
    Ok(())
}

fn ensure_output_is_not_input(input: &Path, output: &Path) -> anyhow::Result<()> {
    let input_abs = absolutize(input);
    let output_abs = absolutize(output);
    if input_abs == output_abs {
        bail!(
            "refusing to overwrite input PDF {}; choose a different -o path",
            input.display()
        );
    }
    Ok(())
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn paper_size(paper: &str) -> anyhow::Result<(f32, f32)> {
    match paper.to_ascii_lowercase().as_str() {
        "a4" => Ok((595.0, 842.0)),
        "letter" => Ok((612.0, 792.0)),
        other => bail!("unsupported paper size `{other}`; expected `a4` or `letter`"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FitMode {
    Contain,
    Cover,
    Stretch,
}

impl FitMode {
    fn parse(input: &str) -> anyhow::Result<Self> {
        match input.to_ascii_lowercase().as_str() {
            "contain" => Ok(Self::Contain),
            "cover" => Ok(Self::Cover),
            "stretch" => Ok(Self::Stretch),
            other => bail!("unsupported fit mode `{other}`; expected contain, cover, or stretch"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fit_modes() {
        assert_eq!(FitMode::parse("contain").unwrap(), FitMode::Contain);
        assert_eq!(FitMode::parse("cover").unwrap(), FitMode::Cover);
        assert_eq!(FitMode::parse("stretch").unwrap(), FitMode::Stretch);
        assert!(FitMode::parse("bad").is_err());
    }

    #[test]
    fn knows_a4_dimensions() {
        assert_eq!(paper_size("a4").unwrap(), (595.0, 842.0));
    }
}
