//! Second-opinion PDF metadata — font names, font weights, struct-tree tags.
//!
//! mupdf 0.6.0's Rust wrapper does not expose per-glyph font names or the
//! tagged-PDF structure tree. Without those signals the classifier is stuck
//! on font-size alone, which misses headings in documents with uniform body
//! font size (common in theses, some journals, any doc emitted without a
//! size hierarchy).
//!
//! This module exposes a small metadata surface the classifier can consult.
//! The default build ships a **stub** — `load_page_metadata` always returns
//! `None` and the classifier falls back to its size-only behaviour, so
//! nothing regresses for users who do not opt in.
//!
//! Building with `--features pdfium-metadata` swaps in a real implementation
//! backed by [`pdfium-render`](https://crates.io/crates/pdfium-render), which
//! dynamically loads `libpdfium` at runtime. See `README.md` for the system
//! requirements.

use crate::document::types::Bbox;

/// A font used to draw part of a page, with its human-readable attributes.
///
/// Not every field is consulted by the current classifier — `family` and
/// `italic` are surfaced for future heuristics and for debugging. They are
/// populated by the real loader but tolerated as dead in the default build.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FontInfo {
    /// Font family name (e.g. "TimesNewRomanPSMT", "Helvetica-Bold").
    pub family: String,
    /// CSS-style weight: 100..=900, where 400 = regular and 700 = bold.
    pub weight: u16,
    pub italic: bool,
}

impl FontInfo {
    /// A weight of 700 or greater is treated as bold for heading purposes.
    pub fn is_bold(&self) -> bool {
        self.weight >= 700
    }
}

/// One entry from a tagged PDF's `/StructTreeRoot`. Maps a logical role
/// (H1, P, Figure, Table, ...) back to the region of the page where it is
/// drawn. `alt` holds any `/Alt` text, which the classifier does not yet
/// consume but format writers may surface in the future.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StructTag {
    pub bbox: Bbox,
    pub role: String,
    pub alt: Option<String>,
}

/// Per-page metadata: the font used at each drawn text region, and any
/// available struct-tree tags.
#[derive(Debug, Default, Clone)]
pub struct PageMetadata {
    /// Bounding box of a text run, paired with the font it was drawn in.
    /// Multiple entries per page are normal (one per font change).
    pub fonts: Vec<(Bbox, FontInfo)>,
    /// Tagged-PDF `StructElement`s that overlap this page.
    pub struct_tags: Vec<StructTag>,
}

impl PageMetadata {
    /// Find the font that best matches `bbox` by geometric overlap.
    ///
    /// The classifier asks for each classified block's font so it can layer
    /// a weight-based heading signal on top of the size ratio.
    pub fn font_for_bbox(&self, bbox: &Bbox) -> Option<&FontInfo> {
        let mut best: Option<(f32, &FontInfo)> = None;
        for (fbbox, font) in &self.fonts {
            let score = bbox_overlap_score(bbox, fbbox);
            if score > 0.10 && best.map(|(b, _)| score > b).unwrap_or(true) {
                best = Some((score, font));
            }
        }
        best.map(|(_, f)| f)
    }

    /// Find a struct-tree role whose region overlaps `bbox` significantly.
    pub fn struct_role_for_bbox(&self, bbox: &Bbox) -> Option<&str> {
        self.struct_tags
            .iter()
            .find(|tag| bbox_overlap_score(bbox, &tag.bbox) > 0.50)
            .map(|tag| tag.role.as_str())
    }
}

/// Overlap score in [0, 1]: intersection over the smaller of the two areas.
///
/// We do not use IoU because a small bold run inside a larger text block is
/// a real match — IoU would punish the size disparity.
fn bbox_overlap_score(a: &Bbox, b: &Bbox) -> f32 {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    if x0 >= x1 || y0 >= y1 {
        return 0.0;
    }
    let intersection = (x1 - x0) * (y1 - y0);
    let smaller = a.area().min(b.area());
    if smaller <= 0.0 {
        0.0
    } else {
        intersection / smaller
    }
}

// ============================================================================
// Loader — stub by default, pdfium-render implementation behind the feature.
// ============================================================================

#[cfg(not(feature = "pdfium-metadata"))]
pub fn load_page_metadata(
    _pdf: &std::path::Path,
    _page_num: usize,
) -> Option<PageMetadata> {
    None
}

#[cfg(feature = "pdfium-metadata")]
pub fn load_page_metadata(
    pdf: &std::path::Path,
    page_num: usize,
) -> Option<PageMetadata> {
    match pdfium_impl::load(pdf, page_num) {
        Ok(md) => Some(md),
        Err(e) => {
            eprintln!(
                "  pdfium-metadata: page {} metadata unavailable ({e})",
                page_num + 1
            );
            None
        }
    }
}

#[cfg(feature = "pdfium-metadata")]
mod pdfium_impl {
    //! Real implementation against `pdfium-render` 0.9.
    //!
    //! Fonts only — struct-tree access is not yet surfaced because
    //! pdfium-render 0.9 does not expose a stable API for it. The feature
    //! still provides the most impactful signal (weight → heading) for
    //! untagged academic PDFs.
    //!
    //! PDF coordinate conversion: pdfium returns bottom-left-origin
    //! coordinates; we flip to top-left by subtracting from page height.

    use super::{FontInfo, PageMetadata};
    use crate::document::types::Bbox;

    use pdfium_render::prelude::*;

    fn weight_to_u16(w: PdfFontWeight) -> u16 {
        match w {
            PdfFontWeight::Weight100 => 100,
            PdfFontWeight::Weight200 => 200,
            PdfFontWeight::Weight300 => 300,
            PdfFontWeight::Weight400Normal => 400,
            PdfFontWeight::Weight500 => 500,
            PdfFontWeight::Weight600 => 600,
            PdfFontWeight::Weight700Bold => 700,
            PdfFontWeight::Weight800 => 800,
            PdfFontWeight::Weight900 => 900,
            PdfFontWeight::Custom(v) => v.clamp(100, 900) as u16,
        }
    }

    pub fn load(
        pdf: &std::path::Path,
        page_num: usize,
    ) -> Result<PageMetadata, String> {
        let bindings = Pdfium::bind_to_system_library()
            .map_err(|e| format!("bind_to_system_library: {e}"))?;
        let pdfium = Pdfium::new(bindings);
        let path_str = pdf.to_str().ok_or_else(|| "non-utf8 PDF path".to_string())?;
        let doc = pdfium
            .load_pdf_from_file(path_str, None)
            .map_err(|e| format!("load_pdf_from_file: {e}"))?;

        let pages = doc.pages();
        let page_idx: std::os::raw::c_int = page_num
            .try_into()
            .map_err(|_| "page_num exceeds c_int::MAX".to_string())?;
        let page = pages.get(page_idx).map_err(|e| format!("get page: {e}"))?;
        let page_height = page.height().value;

        let mut md = PageMetadata::default();

        for object in page.objects().iter() {
            let Some(text) = object.as_text_object() else {
                continue;
            };
            let Ok(quad) = text.bounds() else {
                continue;
            };
            let font = text.font();
            let family = font.family();
            let weight = font.weight().map(weight_to_u16).unwrap_or(400);
            let italic = font.is_italic();

            // Flip y: pdfium bottom-left origin → our top-left origin.
            let l = quad.left().value;
            let r = quad.right().value;
            let y0 = page_height - quad.top().value;
            let y1 = page_height - quad.bottom().value;

            md.fonts.push((
                Bbox::new(l, y0, r, y1),
                FontInfo { family, weight, italic },
            ));
        }

        Ok(md)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn font(family: &str, weight: u16, italic: bool) -> FontInfo {
        FontInfo { family: family.to_string(), weight, italic }
    }

    #[test]
    fn is_bold_threshold() {
        assert!(!font("Times", 400, false).is_bold());
        assert!(!font("Times", 600, false).is_bold());
        assert!(font("Times", 700, false).is_bold());
        assert!(font("Times-Bold", 900, false).is_bold());
    }

    #[test]
    fn font_for_bbox_returns_none_for_empty_metadata() {
        let md = PageMetadata::default();
        assert!(md.font_for_bbox(&Bbox::new(0.0, 0.0, 10.0, 10.0)).is_none());
    }

    #[test]
    fn font_for_bbox_picks_the_containing_run() {
        let mut md = PageMetadata::default();
        md.fonts.push((Bbox::new(0.0, 0.0, 100.0, 20.0), font("Times", 400, false)));
        md.fonts.push((Bbox::new(0.0, 30.0, 100.0, 50.0), font("Helvetica-Bold", 700, false)));

        let heading_bbox = Bbox::new(0.0, 30.0, 100.0, 50.0);
        let chosen = md.font_for_bbox(&heading_bbox).unwrap();
        assert_eq!(chosen.family, "Helvetica-Bold");
        assert!(chosen.is_bold());
    }

    #[test]
    fn font_for_bbox_ignores_non_overlapping() {
        let mut md = PageMetadata::default();
        md.fonts.push((Bbox::new(0.0, 0.0, 100.0, 20.0), font("Times", 400, false)));
        let faraway = Bbox::new(500.0, 500.0, 600.0, 520.0);
        assert!(md.font_for_bbox(&faraway).is_none());
    }

    #[test]
    fn struct_role_match_requires_significant_overlap() {
        let mut md = PageMetadata::default();
        md.struct_tags.push(StructTag {
            bbox: Bbox::new(0.0, 0.0, 100.0, 20.0),
            role: "H1".to_string(),
            alt: None,
        });
        assert_eq!(md.struct_role_for_bbox(&Bbox::new(0.0, 0.0, 100.0, 20.0)), Some("H1"));
        // Tiny sliver overlap — below 0.5 threshold.
        assert!(md
            .struct_role_for_bbox(&Bbox::new(90.0, 0.0, 200.0, 20.0))
            .is_none());
    }

    #[test]
    fn default_loader_returns_none() {
        // With `pdfium-metadata` off (default), the loader always returns None.
        // When the feature is on, it either returns Some(...) or prints a
        // warning and returns None. Either way the default build here asserts
        // the stub behaviour.
        #[cfg(not(feature = "pdfium-metadata"))]
        {
            let md = load_page_metadata(std::path::Path::new("/nonexistent.pdf"), 0);
            assert!(md.is_none());
        }
    }
}
