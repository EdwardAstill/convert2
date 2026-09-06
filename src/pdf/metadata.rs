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
/// drawn. `mcids` exposes the marked-content ids used to derive the region.
/// `alt` and `actual_text` hold `/Alt` and `/ActualText` when Pdfium exposes
/// them, which format writers may surface in the future.
#[derive(Debug, Clone)]
pub struct StructTag {
    pub bbox: Bbox,
    pub role: String,
    pub mcids: Vec<i32>,
    pub alt: Option<String>,
    pub actual_text: Option<String>,
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

    /// Find the most specific struct-tree tag whose region overlaps `bbox`
    /// significantly. When multiple tags match, prefer higher overlap and then
    /// the smaller tag region so broad parent elements do not mask leaf roles.
    pub fn struct_tag_for_bbox(&self, bbox: &Bbox) -> Option<&StructTag> {
        self.struct_tags
            .iter()
            .filter_map(|tag| {
                let score = bbox_overlap_score(bbox, &tag.bbox);
                (score > 0.50).then_some((score, tag.bbox.area(), tag))
            })
            .max_by(|(left_score, left_area, _), (right_score, right_area, _)| {
                left_score
                    .partial_cmp(right_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        right_area
                            .partial_cmp(left_area)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            })
            .map(|(_, _, tag)| tag)
    }

    /// Find a struct-tree role whose region overlaps `bbox` significantly.
    pub fn struct_role_for_bbox(&self, bbox: &Bbox) -> Option<&str> {
        self.struct_tag_for_bbox(bbox).map(|tag| tag.role.as_str())
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
pub fn load_page_metadata(_pdf: &std::path::Path, _page_num: usize) -> Option<PageMetadata> {
    None
}

#[cfg(feature = "pdfium-metadata")]
pub fn load_page_metadata(pdf: &std::path::Path, page_num: usize) -> Option<PageMetadata> {
    static WARNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    match pdfium_impl::load(pdf, page_num) {
        Ok(md) => Some(md),
        Err(e) => {
            WARNED.get_or_init(|| {
                eprintln!(
                    "  pdfium-metadata: font/struct-tree metadata unavailable ({e}); \
                    install libpdfium for richer heading detection"
                );
            });
            None
        }
    }
}

#[cfg(feature = "pdfium-metadata")]
mod pdfium_impl {
    //! Real implementation against `pdfium-render` 0.9.
    //!
    //! This uses the raw FPDF bindings exposed by `pdfium-render` rather than
    //! the high-level page wrapper because the wrapper does not expose
    //! structure-tree handles yet.
    //!
    //! PDF coordinate conversion: pdfium returns bottom-left-origin
    //! coordinates; we flip to top-left by subtracting from page height.

    use super::{FontInfo, PageMetadata, StructTag};
    use crate::document::types::Bbox;

    use pdfium_render::prelude::*;
    use std::collections::{HashMap, HashSet};
    use std::os::raw::{c_double, c_float, c_int};
    use std::ptr;
    use std::sync::OnceLock;

    static PDFIUM_BINDINGS: OnceLock<Result<Box<dyn PdfiumLibraryBindings>, String>> =
        OnceLock::new();

    fn bindings() -> Result<&'static dyn PdfiumLibraryBindings, String> {
        PDFIUM_BINDINGS
            .get_or_init(|| {
                let bindings = Pdfium::bind_to_system_library()
                    .map_err(|e| format!("bind_to_system_library: {e}"))?;
                unsafe {
                    bindings.FPDF_InitLibrary();
                }
                Ok(bindings)
            })
            .as_ref()
            .map(|bindings| bindings.as_ref())
            .map_err(Clone::clone)
    }

    pub fn load(pdf: &std::path::Path, page_num: usize) -> Result<PageMetadata, String> {
        let bindings = bindings()?;
        let path_str = pdf
            .to_str()
            .ok_or_else(|| "non-utf8 PDF path".to_string())?;

        let doc = unsafe { bindings.FPDF_LoadDocument(path_str, None) };
        if doc.is_null() {
            return Err("FPDF_LoadDocument returned null".to_string());
        }

        let page_idx: c_int = page_num
            .try_into()
            .map_err(|_| "page_num exceeds c_int::MAX".to_string())?;
        let page = unsafe { bindings.FPDF_LoadPage(doc, page_idx) };
        if page.is_null() {
            unsafe {
                bindings.FPDF_CloseDocument(doc);
            }
            return Err(format!(
                "FPDF_LoadPage returned null for page {}",
                page_num + 1
            ));
        }

        let page_height = unsafe { bindings.FPDF_GetPageHeightF(page) };

        let mut md = PageMetadata::default();
        let text_page = unsafe { bindings.FPDFText_LoadPage(page) };
        if !text_page.is_null() {
            let char_count = unsafe { bindings.FPDFText_CountChars(text_page) }.max(0);
            for index in 0..char_count {
                let mut left = 0.0 as c_double;
                let mut right = 0.0 as c_double;
                let mut bottom = 0.0 as c_double;
                let mut top = 0.0 as c_double;
                let ok = unsafe {
                    bindings.FPDFText_GetCharBox(
                        text_page,
                        index,
                        &mut left,
                        &mut right,
                        &mut bottom,
                        &mut top,
                    )
                };
                if ok == 0 {
                    continue;
                }
                let Some(bbox) = bbox_from_pdf_coords(
                    left as c_float,
                    bottom as c_float,
                    right as c_float,
                    top as c_float,
                    page_height,
                ) else {
                    continue;
                };

                let mut flags = 0 as c_int;
                let required = unsafe {
                    bindings.FPDFText_GetFontInfo(text_page, index, ptr::null_mut(), 0, &mut flags)
                };
                let family = if required > 0 {
                    let mut buffer = vec![0u8; required as usize];
                    let written = unsafe {
                        bindings.FPDFText_GetFontInfo(
                            text_page,
                            index,
                            buffer.as_mut_ptr().cast(),
                            required,
                            &mut flags,
                        )
                    };
                    if written > 0 {
                        decode_utf8_nul(&buffer).unwrap_or_else(|| "unknown".to_string())
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };

                let raw_weight = unsafe { bindings.FPDFText_GetFontWeight(text_page, index) };
                let family_lower = family.to_ascii_lowercase();
                let weight = if raw_weight > 0 {
                    (raw_weight as u16).clamp(100, 900)
                } else if family_lower.contains("bold") || (flags & 0x40000) != 0 {
                    700
                } else {
                    400
                };
                let italic = (flags & 0x40) != 0
                    || family_lower.contains("italic")
                    || family_lower.contains("oblique");

                md.fonts.push((
                    bbox,
                    FontInfo {
                        family,
                        weight,
                        italic,
                    },
                ));
            }

            unsafe {
                bindings.FPDFText_ClosePage(text_page);
            }
        }

        let mut mcid_bboxes = HashMap::new();
        let object_count = unsafe { bindings.FPDFPage_CountObjects(page) }.max(0);
        for idx in 0..object_count {
            let object = unsafe { bindings.FPDFPage_GetObject(page, idx) };
            if object.is_null() {
                continue;
            }
            let mcid = unsafe { bindings.FPDFPageObj_GetMarkedContentID(object) };
            if mcid < 0 {
                continue;
            }

            let mut left = 0.0 as c_float;
            let mut bottom = 0.0 as c_float;
            let mut right = 0.0 as c_float;
            let mut top = 0.0 as c_float;
            let ok = unsafe {
                bindings.FPDFPageObj_GetBounds(object, &mut left, &mut bottom, &mut right, &mut top)
            };
            if ok == 0 {
                continue;
            }

            let Some(bbox) = bbox_from_pdf_coords(left, bottom, right, top, page_height) else {
                continue;
            };
            mcid_bboxes
                .entry(mcid)
                .and_modify(|existing: &mut Bbox| *existing = existing.union(&bbox))
                .or_insert(bbox);
        }

        if !mcid_bboxes.is_empty() {
            let struct_tree = unsafe { bindings.FPDF_StructTree_GetForPage(page) };
            if !struct_tree.is_null() {
                let mut stack = Vec::new();
                let child_count =
                    unsafe { bindings.FPDF_StructTree_CountChildren(struct_tree) }.max(0);
                for idx in (0..child_count).rev() {
                    let child =
                        unsafe { bindings.FPDF_StructTree_GetChildAtIndex(struct_tree, idx) };
                    if !child.is_null() {
                        stack.push(child);
                    }
                }

                while let Some(element) = stack.pop() {
                    macro_rules! read_struct_string {
                        ($getter:ident) => {{
                            let required = unsafe { bindings.$getter(element, ptr::null_mut(), 0) };
                            if required > 2 {
                                let mut buffer = vec![0u8; required as usize];
                                let written = unsafe {
                                    bindings.$getter(element, buffer.as_mut_ptr().cast(), required)
                                };
                                if written > 0 {
                                    decode_utf16le_nul(&buffer)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }};
                    }

                    let role = read_struct_string!(FPDF_StructElement_GetType);
                    let alt = read_struct_string!(FPDF_StructElement_GetAltText);
                    let actual_text = read_struct_string!(FPDF_StructElement_GetActualText);

                    let mut mcids = HashSet::new();
                    let legacy_mcid =
                        unsafe { bindings.FPDF_StructElement_GetMarkedContentID(element) };
                    if legacy_mcid >= 0 {
                        mcids.insert(legacy_mcid);
                    }

                    let mcid_count =
                        unsafe { bindings.FPDF_StructElement_GetMarkedContentIdCount(element) };
                    if mcid_count > 0 {
                        for idx in 0..mcid_count {
                            let mcid = unsafe {
                                bindings.FPDF_StructElement_GetMarkedContentIdAtIndex(element, idx)
                            };
                            if mcid >= 0 {
                                mcids.insert(mcid);
                            }
                        }
                    }

                    let child_count =
                        unsafe { bindings.FPDF_StructElement_CountChildren(element) }.max(0);
                    for idx in (0..child_count).rev() {
                        let child_mcid = unsafe {
                            bindings.FPDF_StructElement_GetChildMarkedContentID(element, idx)
                        };
                        if child_mcid >= 0 {
                            mcids.insert(child_mcid);
                        }

                        let child =
                            unsafe { bindings.FPDF_StructElement_GetChildAtIndex(element, idx) };
                        if !child.is_null() {
                            stack.push(child);
                        }
                    }

                    let bbox = mcids
                        .iter()
                        .filter_map(|mcid| mcid_bboxes.get(mcid).copied())
                        .reduce(|left, right| left.union(&right));

                    if let (Some(role), Some(bbox)) = (role, bbox) {
                        md.struct_tags.push(StructTag {
                            bbox,
                            role,
                            mcids: {
                                let mut ids: Vec<i32> = mcids.into_iter().collect();
                                ids.sort_unstable();
                                ids
                            },
                            alt,
                            actual_text,
                        });
                    }
                }

                unsafe {
                    bindings.FPDF_StructTree_Close(struct_tree);
                }
            }
        }

        unsafe {
            bindings.FPDF_ClosePage(page);
            bindings.FPDF_CloseDocument(doc);
        }

        Ok(md)
    }

    fn bbox_from_pdf_coords(
        left: c_float,
        bottom: c_float,
        right: c_float,
        top: c_float,
        page_height: c_float,
    ) -> Option<Bbox> {
        let bbox = Bbox::new(left, page_height - top, right, page_height - bottom);
        (bbox.width() > 0.0 && bbox.height() > 0.0).then_some(bbox)
    }

    fn decode_utf8_nul(buffer: &[u8]) -> Option<String> {
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        let text = String::from_utf8_lossy(&buffer[..end]).trim().to_string();
        (!text.is_empty()).then_some(text)
    }

    fn decode_utf16le_nul(buffer: &[u8]) -> Option<String> {
        let mut units: Vec<u16> = buffer
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes(*chunk))
            .collect();
        while units.last().copied() == Some(0) {
            units.pop();
        }
        let text = String::from_utf16(&units).ok()?.trim().to_string();
        (!text.is_empty()).then_some(text)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn font(family: &str, weight: u16, italic: bool) -> FontInfo {
        FontInfo {
            family: family.to_string(),
            weight,
            italic,
        }
    }

    fn tag(bbox: Bbox, role: &str) -> StructTag {
        StructTag {
            bbox,
            role: role.to_string(),
            mcids: Vec::new(),
            alt: None,
            actual_text: None,
        }
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
        md.fonts
            .push((Bbox::new(0.0, 0.0, 100.0, 20.0), font("Times", 400, false)));
        md.fonts.push((
            Bbox::new(0.0, 30.0, 100.0, 50.0),
            font("Helvetica-Bold", 700, false),
        ));

        let heading_bbox = Bbox::new(0.0, 30.0, 100.0, 50.0);
        let chosen = md.font_for_bbox(&heading_bbox).unwrap();
        assert_eq!(chosen.family, "Helvetica-Bold");
        assert!(chosen.is_bold());
    }

    #[test]
    fn font_for_bbox_ignores_non_overlapping() {
        let mut md = PageMetadata::default();
        md.fonts
            .push((Bbox::new(0.0, 0.0, 100.0, 20.0), font("Times", 400, false)));
        let faraway = Bbox::new(500.0, 500.0, 600.0, 520.0);
        assert!(md.font_for_bbox(&faraway).is_none());
    }

    #[test]
    fn struct_role_match_requires_significant_overlap() {
        let mut md = PageMetadata::default();
        md.struct_tags
            .push(tag(Bbox::new(0.0, 0.0, 100.0, 20.0), "H1"));
        assert_eq!(
            md.struct_role_for_bbox(&Bbox::new(0.0, 0.0, 100.0, 20.0)),
            Some("H1")
        );
        // Tiny sliver overlap — below 0.5 threshold.
        assert!(md
            .struct_role_for_bbox(&Bbox::new(90.0, 0.0, 200.0, 20.0))
            .is_none());
    }

    #[test]
    fn struct_role_match_prefers_specific_leaf_tag() {
        let mut md = PageMetadata::default();
        md.struct_tags
            .push(tag(Bbox::new(0.0, 0.0, 500.0, 500.0), "Sect"));
        md.struct_tags
            .push(tag(Bbox::new(50.0, 50.0, 250.0, 75.0), "H2"));

        assert_eq!(
            md.struct_role_for_bbox(&Bbox::new(50.0, 50.0, 250.0, 75.0)),
            Some("H2")
        );
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
