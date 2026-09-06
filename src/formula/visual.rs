use std::path::Path;

use anyhow::Context;
use mupdf::{Colorspace, Device, Document as MuDocument, IRect, Matrix, Pixmap};

use crate::document::types::{Bbox, RawPage};

use super::detect::{FormulaCandidate, FormulaStatus};
use super::ocr::FormulaSidecarAttempt;

const VISUAL_BACKEND: &str = "visual-page-render";
const RENDER_DPI: u32 = 72;
const DARK_THRESHOLD: u8 = 190;

#[derive(Clone, Debug)]
struct DarkBand {
    y0: i32,
    y1: i32,
    x0: i32,
    x1: i32,
    dark_pixels: usize,
    max_horizontal_run: usize,
}

pub fn detect_visual_formula_candidates(
    pdf_path: &Path,
    raw_page: &RawPage,
    existing: &[FormulaCandidate],
    excluded_regions: &[Bbox],
) -> anyhow::Result<Vec<FormulaCandidate>> {
    if is_reference_like_page(raw_page) || (!has_formula_cue(raw_page) && existing.is_empty()) {
        return Ok(Vec::new());
    }

    let document = MuDocument::open(pdf_path).with_context(|| {
        format!(
            "Failed to open {} for visual formula detection",
            pdf_path.display()
        )
    })?;
    let page = document
        .load_page(raw_page.page_num as i32)
        .with_context(|| {
            format!(
                "Failed to load page {} for visual formula detection",
                raw_page.page_num + 1
            )
        })?;
    let pixmap = render_page_pixmap(&page, raw_page.width, raw_page.height, RENDER_DPI)?;
    let scale = RENDER_DPI as f32 / 72.0;
    let page_bbox = raw_page.bbox();

    let mut candidates = Vec::new();
    for band in dark_bands(&pixmap) {
        let bbox = band_to_bbox(&band, scale, page_bbox);
        if !looks_like_formula_band(bbox, raw_page, &band) {
            continue;
        }
        if overlaps_any(bbox, excluded_regions, 0.45)
            || existing
                .iter()
                .any(|candidate| overlap_ratio(bbox, candidate.bbox) > 0.55)
        {
            continue;
        }
        let Some(reason) = visual_reason(raw_page, bbox, &band) else {
            continue;
        };

        candidates.push(FormulaCandidate {
            page_num: raw_page.page_num,
            formula_index: existing.len() + candidates.len(),
            bbox: pad_and_clamp(bbox, 4.0, page_bbox),
            source_text: String::new(),
            equation_number: None,
            confidence: 68,
            status: FormulaStatus::NeedsReview,
            backend: Some(VISUAL_BACKEND.to_string()),
            latex: None,
            words: Vec::new(),
            sidecar: FormulaSidecarAttempt::not_attempted(),
            reason,
            crop_path: None,
        });
    }

    Ok(dedupe_visual_candidates(candidates))
}

fn render_page_pixmap(
    page: &mupdf::Page,
    page_width: f32,
    page_height: f32,
    dpi: u32,
) -> anyhow::Result<Pixmap> {
    let scale = dpi as f32 / 72.0;
    let width = ((page_width * scale).ceil() as i32).max(1);
    let height = ((page_height * scale).ceil() as i32).max(1);
    let mut pixmap = Pixmap::new(&Colorspace::device_rgb(), 0, 0, width, height, false)
        .context("failed to allocate visual formula pixmap")?;
    pixmap
        .clear_with(255)
        .context("failed to clear visual formula pixmap")?;
    {
        let device = Device::from_pixmap_with_clip(&pixmap, IRect::new(0, 0, width, height))
            .context("failed to create visual formula draw device")?;
        page.run(&device, &Matrix::new_scale(scale, scale))
            .context("failed to draw page for visual formula detection")?;
    }
    Ok(pixmap)
}

fn dark_bands(pixmap: &Pixmap) -> Vec<DarkBand> {
    let width = pixmap.width().max(1) as usize;
    let height = pixmap.height().max(1) as usize;
    let mut row_counts = vec![0usize; height];
    let mut row_min_x = vec![width; height];
    let mut row_max_x = vec![0usize; height];
    let mut row_max_run = vec![0usize; height];

    for y in 0..height {
        let start = y * width * 3;
        let end = start + width * 3;
        let Some(row) = pixmap.samples().get(start..end) else {
            break;
        };
        let mut current_run = 0usize;
        for (x, rgb) in row.as_chunks::<3>().0.iter().enumerate() {
            if rgb[0] < DARK_THRESHOLD || rgb[1] < DARK_THRESHOLD || rgb[2] < DARK_THRESHOLD {
                row_counts[y] += 1;
                row_min_x[y] = row_min_x[y].min(x);
                row_max_x[y] = row_max_x[y].max(x);
                current_run += 1;
                row_max_run[y] = row_max_run[y].max(current_run);
            } else {
                current_run = 0;
            }
        }
    }

    let threshold = (width / 90).max(4);
    let mut bands = Vec::new();
    let mut y = 0usize;
    while y < height {
        if smoothed_row_count(&row_counts, y) < threshold {
            y += 1;
            continue;
        }
        let start = y;
        let mut end = y + 1;
        let mut x0 = width;
        let mut x1 = 0usize;
        let mut dark_pixels = 0usize;
        let mut max_horizontal_run = 0usize;
        while end < height && smoothed_row_count(&row_counts, end) >= threshold {
            if row_counts[end] > 0 {
                x0 = x0.min(row_min_x[end]);
                x1 = x1.max(row_max_x[end]);
                dark_pixels += row_counts[end];
                max_horizontal_run = max_horizontal_run.max(row_max_run[end]);
            }
            end += 1;
        }
        if row_counts[start] > 0 {
            x0 = x0.min(row_min_x[start]);
            x1 = x1.max(row_max_x[start]);
            dark_pixels += row_counts[start];
            max_horizontal_run = max_horizontal_run.max(row_max_run[start]);
        }
        if x0 < x1 {
            bands.push(DarkBand {
                y0: start as i32,
                y1: end as i32,
                x0: x0 as i32,
                x1: x1 as i32,
                dark_pixels,
                max_horizontal_run,
            });
        }
        y = end;
    }

    merge_close_bands(bands)
}

fn smoothed_row_count(row_counts: &[usize], y: usize) -> usize {
    let start = y.saturating_sub(1);
    let end = (y + 2).min(row_counts.len());
    row_counts[start..end].iter().sum::<usize>() / (end - start).max(1)
}

fn merge_close_bands(mut bands: Vec<DarkBand>) -> Vec<DarkBand> {
    if bands.is_empty() {
        return bands;
    }
    let mut merged = Vec::new();
    let mut current = bands.remove(0);
    for band in bands {
        if band.y0 - current.y1 <= 6 {
            current.y1 = band.y1;
            current.x0 = current.x0.min(band.x0);
            current.x1 = current.x1.max(band.x1);
            current.dark_pixels += band.dark_pixels;
            current.max_horizontal_run = current.max_horizontal_run.max(band.max_horizontal_run);
        } else {
            merged.push(current);
            current = band;
        }
    }
    merged.push(current);
    merged
}

fn band_to_bbox(band: &DarkBand, scale: f32, page_bbox: Bbox) -> Bbox {
    Bbox::new(
        (band.x0 as f32 / scale).max(page_bbox.x0),
        (band.y0 as f32 / scale).max(page_bbox.y0),
        (band.x1 as f32 / scale).min(page_bbox.x1),
        (band.y1 as f32 / scale).min(page_bbox.y1),
    )
}

/// Minimum band height in pixels to qualify as a formula region.
/// Derived from RENDER_DPI so the threshold scales if resolution changes.
/// Targets ~4pt (~1.4mm) at any DPI. Body text glyphs are 8–14px; decorative rules 1–3px.
const MIN_BAND_HEIGHT_PX: i32 = 4 * RENDER_DPI as i32 / 72;

fn band_has_sufficient_height(band: &DarkBand) -> bool {
    (band.y1 - band.y0) >= MIN_BAND_HEIGHT_PX
}

fn looks_like_formula_band(bbox: Bbox, raw_page: &RawPage, band: &DarkBand) -> bool {
    if !band_has_sufficient_height(band) {
        return false;
    }
    let height = bbox.height();
    let width = bbox.width();
    if !(8.0..=90.0).contains(&height) || width < raw_page.width * 0.12 {
        return false;
    }
    if width > raw_page.width * 0.92 && height > 45.0 {
        return false;
    }
    let density =
        band.dark_pixels as f32 / ((band.x1 - band.x0).max(1) * (band.y1 - band.y0).max(1)) as f32;
    if !(0.015..=0.55).contains(&density) {
        return false;
    }
    let has_fraction_rule = band.max_horizontal_run as f32 >= raw_page.width * 0.12;
    let text_overlap = raw_page
        .words
        .iter()
        .filter(|word| overlap_ratio(bbox, word.bbox) > 0.20)
        .count();
    text_overlap <= 3 || has_fraction_rule
}

fn visual_reason(raw_page: &RawPage, bbox: Bbox, band: &DarkBand) -> Option<String> {
    let cue = nearest_formula_cue(raw_page, bbox);
    let centered = (bbox.center_x() - raw_page.width / 2.0).abs() < raw_page.width * 0.30;
    let isolated = nearest_word_gap(raw_page, bbox) > 12.0;
    let has_fraction_rule = band.max_horizontal_run as f32 >= raw_page.width * 0.12;

    if cue.is_some() || (centered && (isolated || has_fraction_rule)) {
        let mut reasons = vec!["visual-isolated-equation-band".to_string()];
        if let Some(cue) = cue {
            reasons.push(format!("cue:{cue}"));
        }
        if centered {
            reasons.push("centered".to_string());
        }
        if isolated {
            reasons.push("text-gap".to_string());
        }
        if has_fraction_rule {
            reasons.push("horizontal-rule".to_string());
        }
        return Some(reasons.join("+"));
    }
    None
}

fn nearest_formula_cue(raw_page: &RawPage, bbox: Bbox) -> Option<String> {
    raw_page.blocks.iter().find_map(|block| {
        let text = block.text.trim();
        if !contains_formula_cue(text) {
            return None;
        }
        let vertical_gap = if block.bbox.y1 <= bbox.y0 {
            bbox.y0 - block.bbox.y1
        } else if bbox.y1 <= block.bbox.y0 {
            block.bbox.y0 - bbox.y1
        } else {
            0.0
        };
        (vertical_gap <= 140.0).then(|| text.chars().take(48).collect())
    })
}

fn has_formula_cue(raw_page: &RawPage) -> bool {
    raw_page
        .blocks
        .iter()
        .any(|block| contains_formula_cue(&block.text))
}

fn contains_formula_cue(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "hence",
        "where:",
        "given by",
        "calculated as",
        "formula",
        "equation",
        "shall be taken as",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

fn nearest_word_gap(raw_page: &RawPage, bbox: Bbox) -> f32 {
    raw_page
        .words
        .iter()
        .filter_map(|word| {
            if word.bbox.x1 < bbox.x0 || word.bbox.x0 > bbox.x1 {
                return None;
            }
            if word.bbox.y1 <= bbox.y0 {
                Some(bbox.y0 - word.bbox.y1)
            } else if bbox.y1 <= word.bbox.y0 {
                Some(word.bbox.y0 - bbox.y1)
            } else {
                Some(0.0)
            }
        })
        .fold(f32::MAX, f32::min)
}

fn is_reference_like_page(raw_page: &RawPage) -> bool {
    let text = raw_page
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let lower = text.to_ascii_lowercase();
    let reference_lines = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('/')
                || trimmed.starts_with('[')
                || trimmed
                    .chars()
                    .take(4)
                    .any(|ch| ch.is_ascii_digit() || ch == '/')
        })
        .count();
    lower.contains("references") && reference_lines >= 5
}

fn overlaps_any(bbox: Bbox, regions: &[Bbox], threshold: f32) -> bool {
    regions
        .iter()
        .any(|region| overlap_ratio(bbox, *region) > threshold)
}

fn dedupe_visual_candidates(candidates: Vec<FormulaCandidate>) -> Vec<FormulaCandidate> {
    let mut kept: Vec<FormulaCandidate> = Vec::new();
    for candidate in candidates {
        if kept
            .iter()
            .any(|existing| overlap_ratio(existing.bbox, candidate.bbox) > 0.60)
        {
            continue;
        }
        kept.push(candidate);
    }
    kept.into_iter()
        .enumerate()
        .map(|(idx, mut candidate)| {
            candidate.formula_index = idx;
            candidate
        })
        .collect()
}

fn pad_and_clamp(bbox: Bbox, padding: f32, page_bbox: Bbox) -> Bbox {
    Bbox::new(
        (bbox.x0 - padding).max(page_bbox.x0),
        (bbox.y0 - padding).max(page_bbox.y0),
        (bbox.x1 + padding).min(page_bbox.x1),
        (bbox.y1 + padding).min(page_bbox.y1),
    )
}

fn overlap_ratio(a: Bbox, b: Bbox) -> f32 {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    let intersection = ((x1 - x0).max(0.0)) * ((y1 - y0).max(0.0));
    intersection / a.area().min(b.area()).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::types::{RawTextBlock, RawWord};

    fn raw_page(block_text: &str) -> RawPage {
        RawPage {
            page_num: 0,
            width: 600.0,
            height: 800.0,
            blocks: vec![RawTextBlock {
                bbox: Bbox::new(80.0, 320.0, 240.0, 340.0),
                text: block_text.to_string(),
                font_size: 10.0,
                font_name: String::new(),
                page_num: 0,
                block_id: 0,
                reading_order: 0,
            }],
            words: Vec::new(),
            image_refs: Vec::new(),
        }
    }

    #[test]
    fn cue_near_band_creates_visual_reason() {
        let page = raw_page("Hence:");
        let bbox = Bbox::new(180.0, 360.0, 420.0, 386.0);

        let band = DarkBand {
            y0: 360,
            y1: 386,
            x0: 180,
            x1: 420,
            dark_pixels: 300,
            max_horizontal_run: 24,
        };

        let reason = visual_reason(&page, bbox, &band).expect("cue should create visual reason");

        assert!(reason.contains("visual-isolated-equation-band"));
        assert!(reason.contains("cue:Hence:"));
    }

    #[test]
    fn page_without_formula_cue_can_skip_visual_rendering() {
        let page = raw_page("Ordinary paragraph text.");

        assert!(!has_formula_cue(&page));
    }

    #[test]
    fn centered_horizontal_rule_creates_visual_reason_without_same_page_cue() {
        let page = raw_page("Ordinary paragraph text.");
        let bbox = Bbox::new(120.0, 92.0, 600.0, 113.0);
        let band = DarkBand {
            y0: 92,
            y1: 113,
            x0: 120,
            x1: 600,
            dark_pixels: 1200,
            max_horizontal_run: 143,
        };

        let reason =
            visual_reason(&page, bbox, &band).expect("fraction rule should create visual reason");

        assert!(reason.contains("horizontal-rule"));
    }

    #[test]
    fn references_page_is_suppressed() {
        let page = raw_page("References\n/34/ DNV-RU-OU-0300\n/35/ DNV-ST-N001\n/36/ ISO 19901\n/37/ API RP 2A\n/38/ EN 1993");

        assert!(is_reference_like_page(&page));
    }

    #[test]
    fn band_below_height_threshold_rejected() {
        let band = DarkBand {
            y0: 100,
            y1: 101, // 1px — decorative rule
            x0: 50,
            x1: 540,
            dark_pixels: 490,
            max_horizontal_run: 490,
        };
        assert!(
            !band_has_sufficient_height(&band),
            "1px band should fail height check"
        );
    }

    #[test]
    fn band_with_glyph_height_accepted() {
        let band = DarkBand {
            y0: 100,
            y1: 115, // 15px — normal formula glyph
            x0: 150,
            x1: 400,
            dark_pixels: 200,
            max_horizontal_run: 50,
        };
        assert!(
            band_has_sufficient_height(&band),
            "15px band should pass height check"
        );
    }

    #[test]
    fn overlapping_words_reject_formula_band() {
        let mut page = raw_page("Hence:");
        page.words = vec![
            RawWord {
                bbox: Bbox::new(190.0, 362.0, 220.0, 374.0),
                text: "text".into(),
                font_size: 10.0,
                page_num: 0,
                block_id: 1,
                line_id: 0,
                baseline_y: 374.0,
            },
            RawWord {
                bbox: Bbox::new(230.0, 362.0, 260.0, 374.0),
                text: "text".into(),
                font_size: 10.0,
                page_num: 0,
                block_id: 1,
                line_id: 0,
                baseline_y: 374.0,
            },
            RawWord {
                bbox: Bbox::new(270.0, 362.0, 300.0, 374.0),
                text: "text".into(),
                font_size: 10.0,
                page_num: 0,
                block_id: 1,
                line_id: 0,
                baseline_y: 374.0,
            },
            RawWord {
                bbox: Bbox::new(310.0, 362.0, 340.0, 374.0),
                text: "text".into(),
                font_size: 10.0,
                page_num: 0,
                block_id: 1,
                line_id: 0,
                baseline_y: 374.0,
            },
        ];
        let band = DarkBand {
            y0: 360,
            y1: 386,
            x0: 180,
            x1: 420,
            dark_pixels: 300,
            max_horizontal_run: 24,
        };

        assert!(!looks_like_formula_band(
            Bbox::new(180.0, 360.0, 420.0, 386.0),
            &page,
            &band
        ));
    }
}
