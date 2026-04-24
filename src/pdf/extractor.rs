use std::collections::HashMap;
use std::path::Path;

use mupdf::text_page::TextBlockType;
use mupdf::{Document, ImageFormat, MetadataName, TextPageFlags};

use crate::document::types::{Bbox, DocumentMetadata, ImageRef, RawPage, RawTextBlock};
use crate::error::{VtvError, VtvResult};

#[path = "text_cleanup.rs"]
mod text_cleanup;

use self::text_cleanup::cleanup_extracted_text;

pub struct PdfExtractor;

impl PdfExtractor {
    /// Extract all pages from a PDF file.
    #[allow(dead_code)]
    pub fn extract_pages(path: &Path) -> VtvResult<Vec<RawPage>> {
        let path_str = path.to_string_lossy();
        let doc = Document::open(path_str.as_ref()).map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let page_count = doc.page_count().map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let mut pages = Vec::with_capacity(page_count as usize);
        for i in 0..page_count {
            let page = Self::extract_page(&doc, i as usize)?;
            pages.push(page);
        }
        Ok(pages)
    }

    /// Extract document metadata (title, author, subject, page count).
    #[allow(dead_code)]
    pub fn extract_metadata(path: &Path) -> VtvResult<DocumentMetadata> {
        let path_str = path.to_string_lossy();
        let doc = Document::open(path_str.as_ref()).map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let page_count = doc.page_count().map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })? as usize;

        let title = doc.metadata(MetadataName::Title).ok().and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

        let author = doc.metadata(MetadataName::Author).ok().and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

        let subject = doc.metadata(MetadataName::Subject).ok().and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

        Ok(DocumentMetadata {
            title,
            author,
            subject,
            page_count,
        })
    }

    /// Extract all pages and metadata from a PDF in a single file open.
    pub fn extract(path: &Path) -> VtvResult<(Vec<RawPage>, DocumentMetadata)> {
        let path_str = path.to_string_lossy();
        let doc = Document::open(path_str.as_ref()).map_err(|e| VtvError::PdfOpen {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let page_count = doc.page_count().map_err(|e| VtvError::PdfExtraction {
            page: 0,
            message: e.to_string(),
        })? as usize;

        let mut pages = Vec::with_capacity(page_count);
        for page_num in 0..page_count {
            pages.push(Self::extract_page(&doc, page_num)?);
        }

        // Extract metadata from the already-open document
        let metadata = DocumentMetadata {
            title: doc
                .metadata(MetadataName::Title)
                .ok()
                .filter(|s| !s.is_empty()),
            author: doc
                .metadata(MetadataName::Author)
                .ok()
                .filter(|s| !s.is_empty()),
            subject: doc
                .metadata(MetadataName::Subject)
                .ok()
                .filter(|s| !s.is_empty()),
            page_count,
        };

        Ok((pages, metadata))
    }

    // --- private helpers ---

    fn extract_page(doc: &Document, page_num: usize) -> VtvResult<RawPage> {
        let page = doc
            .load_page(page_num as i32)
            .map_err(|e| VtvError::PdfExtraction {
                page: page_num,
                message: e.to_string(),
            })?;

        let bounds = page.bounds().map_err(|e| VtvError::PdfExtraction {
            page: page_num,
            message: e.to_string(),
        })?;

        let text_page = page
            .to_text_page(TextPageFlags::PRESERVE_IMAGES)
            .map_err(|e| VtvError::PdfExtraction {
                page: page_num,
                message: e.to_string(),
            })?;

        let mut blocks: Vec<RawTextBlock> = Vec::new();
        let mut image_refs: Vec<ImageRef> = Vec::new();
        let mut image_index: usize = 0;

        for (block_id, block) in text_page.blocks().enumerate() {
            match block.r#type() {
                TextBlockType::Text => {
                    let text = cleanup_extracted_text(&Self::collect_block_text(&block));
                    if text.is_empty() {
                        continue;
                    }
                    let font_size = Self::dominant_font_size(&block);
                    let font_name = Self::dominant_font_name(&block);
                    let bbox = Self::mupdf_rect_to_bbox(block.bounds());

                    blocks.push(RawTextBlock {
                        bbox,
                        text,
                        font_size,
                        font_name,
                        page_num,
                        block_id,
                        reading_order: 0,
                    });
                }
                TextBlockType::Image => {
                    let bbox = Self::mupdf_rect_to_bbox(block.bounds());
                    // Decode the image block's pixmap to PNG bytes. Silently skip
                    // on any failure — an unreadable image is strictly better to
                    // drop than to panic the whole extraction.
                    if let Some(image) = block.image() {
                        if let Ok(pixmap) = image.to_pixmap() {
                            let mut bytes: Vec<u8> = Vec::new();
                            if pixmap.write_to(&mut bytes, ImageFormat::PNG).is_ok()
                                && !bytes.is_empty()
                            {
                                image_refs.push(ImageRef {
                                    page_num,
                                    bbox,
                                    image_index,
                                    bytes,
                                    format: "png".to_string(),
                                });
                                image_index += 1;
                            }
                        }
                    }
                }
                // Ignore Struct, Vector, Grid block types
                _ => {}
            }
        }

        Ok(RawPage {
            page_num,
            width: bounds.x1 - bounds.x0,
            height: bounds.y1 - bounds.y0,
            blocks,
            image_refs,
        })
    }

    fn mupdf_rect_to_bbox(r: mupdf::Rect) -> Bbox {
        Bbox::new(r.x0, r.y0, r.x1, r.y1)
    }

    /// Collect all text from a TextBlock's lines into a single string.
    /// Detects subscript/superscript characters by comparing each character's
    /// baseline position and font size against the dominant values,
    /// and emits `_{...}` / `^{...}` wrappers for runs of sub/superscripts.
    ///
    /// For normal horizontal text, groups mupdf lines into "text rows" by
    /// y-position to fix reading order when mupdf puts subscripts on separate
    /// lines. For vertical text (watermarks, rotated text), falls back to
    /// per-mupdf-line processing to avoid one-char-per-row bloat.
    fn collect_block_text(block: &mupdf::TextBlock<'_>) -> String {
        let block_dominant_size = Self::dominant_font_size(block);

        // Collect all characters from all lines with their positions.
        // Also track which mupdf line each char came from (for fallback).
        let mut all_chars: Vec<CharInfo> = Vec::new();
        let mut line_boundaries: Vec<usize> = Vec::new(); // start index of each mupdf line
        for line in block.lines() {
            line_boundaries.push(all_chars.len());
            for ch in line.chars() {
                let origin = ch.origin();
                all_chars.push(CharInfo {
                    ch: ch.char(),
                    size: ch.size(),
                    origin_x: origin.x,
                    origin_y: origin.y,
                });
            }
        }

        // Filter to visible chars only for grouping.
        let visible_count = all_chars.iter().filter(|c| c.ch.is_some()).count();
        if visible_count == 0 {
            return String::new();
        }

        // Detect vertical/rotated text: if regrouping by y-position would
        // create too many single-character rows, the text is likely vertical
        // (e.g., DRM watermarks). Fall back to per-mupdf-line processing.
        let is_vertical = is_vertical_text(&all_chars, block_dominant_size);

        if is_vertical {
            // Fallback: process each mupdf line independently with subscript
            // detection within each line, but don't regroup across lines.
            return Self::collect_block_text_per_line(block, block_dominant_size);
        }

        // Normal path: regroup across mupdf lines for correct reading order.
        let rows = group_into_text_rows(&all_chars, block_dominant_size);

        let mut result_lines: Vec<String> = Vec::new();

        for row in &rows {
            if row.is_empty() {
                continue;
            }

            // Find the dominant baseline for normal-sized chars in this row.
            let row_baseline =
                largest_char_baseline(row, block_dominant_size).unwrap_or_else(|| {
                    let sum: f32 = row
                        .iter()
                        .filter(|c| c.ch.is_some())
                        .map(|c| c.origin_y)
                        .sum();
                    let count = row.iter().filter(|c| c.ch.is_some()).count();
                    if count > 0 {
                        sum / count as f32
                    } else {
                        0.0
                    }
                });

            let line_str =
                build_line_with_scripts_from_info(row, block_dominant_size, row_baseline);

            let trimmed = line_str.trim_end().to_owned();
            if !trimmed.is_empty() {
                result_lines.push(trimmed);
            }
        }

        result_lines.join("\n")
    }

    /// Fallback text collection for vertical/rotated text blocks.
    /// Processes each mupdf line independently with subscript detection
    /// but does not regroup characters across lines.
    fn collect_block_text_per_line(
        block: &mupdf::TextBlock<'_>,
        block_dominant_size: f32,
    ) -> String {
        let mut lines_text: Vec<String> = Vec::new();

        for line in block.lines() {
            let chars: Vec<CharInfo> = line
                .chars()
                .map(|ch| {
                    let origin = ch.origin();
                    CharInfo {
                        ch: ch.char(),
                        size: ch.size(),
                        origin_x: origin.x,
                        origin_y: origin.y,
                    }
                })
                .filter(|c| c.ch.is_some())
                .collect();

            if chars.is_empty() {
                continue;
            }

            let baseline =
                largest_char_baseline(&chars, block_dominant_size).unwrap_or_else(|| {
                    let sum: f32 = chars.iter().map(|c| c.origin_y).sum();
                    sum / chars.len() as f32
                });

            let line_str = build_line_with_scripts_from_info(&chars, block_dominant_size, baseline);
            let trimmed = line_str.trim_end().to_owned();
            if !trimmed.is_empty() {
                lines_text.push(trimmed);
            }
        }

        lines_text.join("\n")
    }

    /// Get the dominant font size in a TextBlock (mode of char sizes).
    fn dominant_font_size(block: &mupdf::TextBlock<'_>) -> f32 {
        // Collect sizes bucketed to 1 decimal place to handle floating point variation
        let mut counts: HashMap<u32, (usize, f32)> = HashMap::new();

        for line in block.lines() {
            for ch in line.chars() {
                let size = ch.size();
                // Key by rounded-to-nearest-tenth: multiply by 10, round, cast to u32
                let key = (size * 10.0).round() as u32;
                let entry = counts.entry(key).or_insert((0, size));
                entry.0 += 1;
            }
        }

        if counts.is_empty() {
            return 12.0;
        }

        counts
            .into_values()
            .max_by_key(|(count, _)| *count)
            .map(|(_, size)| size)
            .unwrap_or(12.0)
    }

    /// Get the dominant font name in a TextBlock.
    /// Note: the mupdf 0.6.0 Rust wrapper does not expose per-char font name,
    /// so this always returns "unknown".
    fn dominant_font_name(_block: &mupdf::TextBlock<'_>) -> String {
        // The mupdf::TextChar API only exposes char(), origin(), size(), quad().
        // Font name requires direct fz_stext_char.style->font access which is not
        // surfaced by the wrapper. Return "unknown" as specified fallback.
        "unknown".to_owned()
    }
}

/// Detect vertical/rotated text by counting how many unique y-baselines
/// exist among normal-sized characters. In horizontal text, many chars share
/// the same y-baseline (one per line). In vertical text, each char has its
/// own unique y, producing as many baselines as characters.
fn is_vertical_text(all_chars: &[CharInfo], block_dominant_size: f32) -> bool {
    let size_threshold = block_dominant_size * 0.80;
    let normal_chars: Vec<&CharInfo> = all_chars
        .iter()
        .filter(|c| c.ch.is_some() && c.size >= size_threshold)
        .collect();

    if normal_chars.len() < 4 {
        return false; // too few chars to judge
    }

    // Count unique baselines (y-positions within 0.5 * dominant size)
    let mut baselines: Vec<f32> = Vec::new();
    for ci in &normal_chars {
        let found = baselines
            .iter()
            .any(|&b| (b - ci.origin_y).abs() < block_dominant_size * 0.5);
        if !found {
            baselines.push(ci.origin_y);
        }
    }

    // If more than half the normal chars are on unique baselines, it's vertical text.
    // Normal horizontal text: ~5-40 baselines for 100+ chars (many chars per line).
    // Vertical text: ~N baselines for N chars (one char per line).
    baselines.len() > normal_chars.len() / 2
}

/// Group all characters in a block into text rows.
/// A text row contains all characters (including subscripts/superscripts) that
/// visually belong on the same line of text. Characters are assigned to a row
/// based on the y-position of normal-sized characters. Small characters (subscripts)
/// are assigned to the row whose normal-sized characters they are closest to vertically.
/// Within each row, characters are sorted by x-position for correct reading order.
fn group_into_text_rows(all_chars: &[CharInfo], block_dominant_size: f32) -> Vec<Vec<CharInfo>> {
    if all_chars.is_empty() {
        return Vec::new();
    }

    // First, identify row baselines from normal-sized characters.
    // A normal-sized char has size >= 80% of block dominant.
    let size_threshold = block_dominant_size * 0.80;
    let mut row_baselines: Vec<f32> = Vec::new();

    for ci in all_chars
        .iter()
        .filter(|c| c.ch.is_some() && c.size >= size_threshold)
    {
        let y = ci.origin_y;
        // Check if this y is close to an existing row baseline.
        let found = row_baselines
            .iter()
            .any(|&b| (b - y).abs() < block_dominant_size * 0.5);
        if !found {
            row_baselines.push(y);
        }
    }

    // Sort row baselines top to bottom.
    row_baselines.sort_by(|a, b| a.partial_cmp(b).unwrap());

    if row_baselines.is_empty() {
        // No normal-sized chars — treat all chars as one row.
        let mut row: Vec<CharInfo> = all_chars
            .iter()
            .filter(|c| c.ch.is_some())
            .cloned()
            .collect();
        row.sort_by(|a, b| a.origin_x.partial_cmp(&b.origin_x).unwrap());
        return vec![row];
    }

    // Assign each character to the nearest row baseline.
    let mut rows: Vec<Vec<CharInfo>> = vec![Vec::new(); row_baselines.len()];

    for ci in all_chars.iter().filter(|c| c.ch.is_some()) {
        let mut best_row = 0;
        let mut best_dist = f32::MAX;
        for (i, &baseline) in row_baselines.iter().enumerate() {
            let dist = (ci.origin_y - baseline).abs();
            if dist < best_dist {
                best_dist = dist;
                best_row = i;
            }
        }
        rows[best_row].push(ci.clone());
    }

    // Sort each row by x-position for correct reading order.
    for row in &mut rows {
        row.sort_by(|a, b| a.origin_x.partial_cmp(&b.origin_x).unwrap());
    }

    rows
}

/// Find the baseline y-position of the largest character(s) in a line.
/// Used for short lines where the "dominant" metrics might be from subscript chars.
/// Returns the baseline of chars whose size is >= 80% of the block dominant size.
fn largest_char_baseline(chars: &[CharInfo], block_dominant_size: f32) -> Option<f32> {
    let mut baseline_sum = 0.0f32;
    let mut count = 0usize;
    for ci in chars {
        if ci.ch.is_none() {
            continue;
        }
        if ci.size / block_dominant_size >= 0.80 {
            baseline_sum += ci.origin_y;
            count += 1;
        }
    }
    if count > 0 {
        Some(baseline_sum / count as f32)
    } else {
        None
    }
}

/// Extracted character data — avoids lifetime issues with mupdf::TextChar.
#[derive(Debug, Clone)]
struct CharInfo {
    ch: Option<char>,
    size: f32,
    origin_x: f32,
    origin_y: f32,
}

/// Build a line string with `_{...}` / `^{...}` markers from CharInfo data.
/// Detects horizontal gaps between consecutive sub/superscript characters to
/// break them into separate runs (e.g., `_{c}P_{n}` instead of `_{c n}`).
fn build_line_with_scripts_from_info(
    chars: &[CharInfo],
    dominant_size: f32,
    dominant_baseline: f32,
) -> String {
    let mut result = String::new();
    let mut current_script = ScriptKind::Normal;
    let mut script_buf = String::new();
    let mut prev_x: Option<f32> = None;
    let mut prev_size: Option<f32> = None;

    for ci in chars {
        let c = match ci.ch {
            Some(c) => c,
            None => continue,
        };

        let kind = classify_char_script(ci.size, ci.origin_y, dominant_size, dominant_baseline);

        // If we're continuing a sub/superscript run, check for a horizontal gap
        // that would indicate these subscripts belong to different parent symbols.
        if kind == current_script && kind != ScriptKind::Normal {
            if let (Some(px), Some(ps)) = (prev_x, prev_size) {
                let x_gap = ci.origin_x - px;
                // If the gap is larger than ~1.5x the char width (estimated from size),
                // break the run. This handles cases like "φ_c P_n" where c and n are
                // on the same sub-line but far apart horizontally.
                let char_width_est = ps * 0.6; // rough estimate of char width
                if x_gap > char_width_est * 1.5 {
                    flush_script_run(&mut result, &script_buf, current_script);
                    script_buf.clear();
                    // The gap likely had a normal-size char between them that was
                    // on a different mupdf line. We don't insert anything here —
                    // the normal-size char will appear when its line is processed.
                }
            }
        }

        // If we're in a sub/superscript run and encounter a space, treat it
        // as a run boundary. Subscript spaces are almost always artifacts from
        // equation layout where subscripts of different parent symbols happen
        // to be on the same mupdf line.
        if kind != ScriptKind::Normal && c == ' ' {
            flush_script_run(&mut result, &script_buf, current_script);
            script_buf.clear();
            current_script = ScriptKind::Normal;
            // Don't push the space — it's an artifact.
            prev_x = Some(ci.origin_x);
            prev_size = Some(ci.size);
            continue;
        }

        if kind != current_script {
            flush_script_run(&mut result, &script_buf, current_script);
            script_buf.clear();
            current_script = kind;
        }
        script_buf.push(c);
        prev_x = Some(ci.origin_x);
        prev_size = Some(ci.size);
    }

    flush_script_run(&mut result, &script_buf, current_script);

    result
}

/// Classification of a character's vertical position relative to the line baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ScriptKind {
    Normal,
    Subscript,
    Superscript,
}

/// Classify a character as normal, subscript, or superscript based on its
/// font size and baseline position relative to the line's dominant values.
fn classify_char_script(
    size: f32,
    origin_y: f32,
    dominant_size: f32,
    dominant_baseline: f32,
) -> ScriptKind {
    // Only consider characters that are significantly smaller than the dominant size.
    let size_ratio = size / dominant_size;
    if size_ratio >= 0.80 {
        return ScriptKind::Normal;
    }

    let y_shift = origin_y - dominant_baseline;
    // Threshold scales with font size: ~15% of dominant size, minimum 1pt.
    // For 10pt text this is 1.5pt; for 7pt text, 1.05pt.
    let threshold = (dominant_size * 0.15).max(1.0);

    if y_shift > threshold {
        // y increases downward, so shifted down = subscript
        ScriptKind::Subscript
    } else if y_shift < -threshold {
        ScriptKind::Superscript
    } else {
        // Small size but no significant baseline shift — still mark based on
        // a more lenient threshold when the size difference is very large.
        if size_ratio < 0.65 {
            let lenient_threshold = threshold * 0.33;
            if y_shift > lenient_threshold {
                ScriptKind::Subscript
            } else if y_shift < -lenient_threshold {
                ScriptKind::Superscript
            } else {
                ScriptKind::Normal
            }
        } else {
            ScriptKind::Normal
        }
    }
}

/// Flush a run of characters, wrapping with `_{...}` or `^{...}` if needed.
fn flush_script_run(result: &mut String, buf: &str, kind: ScriptKind) {
    if buf.is_empty() {
        return;
    }
    match kind {
        ScriptKind::Normal => result.push_str(buf),
        ScriptKind::Subscript => {
            result.push_str("_{");
            result.push_str(buf);
            result.push('}');
        }
        ScriptKind::Superscript => {
            result.push_str("^{");
            result.push_str(buf);
            result.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_char_script_normal() {
        // Same size, same baseline → Normal
        assert_eq!(
            classify_char_script(10.0, 100.0, 10.0, 100.0),
            ScriptKind::Normal
        );
    }

    #[test]
    fn test_classify_char_script_subscript() {
        // Smaller size (70% of dominant), shifted down by 3pt → Subscript
        assert_eq!(
            classify_char_script(7.0, 103.0, 10.0, 100.0),
            ScriptKind::Subscript
        );
    }

    #[test]
    fn test_classify_char_script_superscript() {
        // Smaller size (70% of dominant), shifted up by 3pt → Superscript
        assert_eq!(
            classify_char_script(7.0, 97.0, 10.0, 100.0),
            ScriptKind::Superscript
        );
    }

    #[test]
    fn test_classify_char_script_small_but_no_shift() {
        // Smaller size but no significant shift → Normal
        assert_eq!(
            classify_char_script(7.5, 100.5, 10.0, 100.0),
            ScriptKind::Normal
        );
    }

    #[test]
    fn test_classify_char_script_large_at_different_position() {
        // Same size as dominant but shifted — not a sub/superscript, just layout
        assert_eq!(
            classify_char_script(10.0, 105.0, 10.0, 100.0),
            ScriptKind::Normal
        );
    }

    #[test]
    fn test_classify_char_script_borderline_size() {
        // 81% of dominant size — just above threshold, should be Normal
        assert_eq!(
            classify_char_script(8.1, 103.0, 10.0, 100.0),
            ScriptKind::Normal
        );
    }

    #[test]
    fn test_classify_char_script_very_small_subtle_shift() {
        // Very small (60% of dominant), slight downward shift → Subscript
        assert_eq!(
            classify_char_script(6.0, 101.0, 10.0, 100.0),
            ScriptKind::Subscript
        );
    }

    #[test]
    fn test_flush_script_run_normal() {
        let mut result = String::new();
        flush_script_run(&mut result, "hello", ScriptKind::Normal);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_flush_script_run_subscript() {
        let mut result = String::new();
        flush_script_run(&mut result, "n", ScriptKind::Subscript);
        assert_eq!(result, "_{n}");
    }

    #[test]
    fn test_flush_script_run_superscript() {
        let mut result = String::new();
        flush_script_run(&mut result, "2", ScriptKind::Superscript);
        assert_eq!(result, "^{2}");
    }

    #[test]
    fn test_flush_script_run_empty() {
        let mut result = String::new();
        flush_script_run(&mut result, "", ScriptKind::Subscript);
        assert_eq!(result, "");
    }

    #[test]
    fn test_flush_script_run_grouped() {
        let mut result = String::new();
        flush_script_run(&mut result, "abc", ScriptKind::Subscript);
        assert_eq!(result, "_{abc}");
    }

    #[test]
    fn test_classify_aisc_subscript_borderline_shift() {
        // Real-world case from AISC 360-2022: 9.5pt dominant, subscript c at 7pt
        // with 1.95pt downward shift. Threshold = 9.5 * 0.15 = 1.425pt.
        // 1.95 > 1.425 so this should be detected as subscript.
        assert_eq!(
            classify_char_script(7.0, 373.97, 9.5, 372.02),
            ScriptKind::Subscript
        );
    }

    #[test]
    fn test_group_into_text_rows_single_row() {
        let chars = vec![
            CharInfo {
                ch: Some('H'),
                size: 10.0,
                origin_x: 0.0,
                origin_y: 100.0,
            },
            CharInfo {
                ch: Some('i'),
                size: 10.0,
                origin_x: 6.0,
                origin_y: 100.0,
            },
        ];
        let rows = group_into_text_rows(&chars, 10.0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 2);
    }

    #[test]
    fn test_group_into_text_rows_two_rows() {
        let chars = vec![
            CharInfo {
                ch: Some('A'),
                size: 10.0,
                origin_x: 0.0,
                origin_y: 100.0,
            },
            CharInfo {
                ch: Some('B'),
                size: 10.0,
                origin_x: 0.0,
                origin_y: 120.0,
            },
        ];
        let rows = group_into_text_rows(&chars, 10.0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].ch, Some('A'));
        assert_eq!(rows[1][0].ch, Some('B'));
    }

    #[test]
    fn test_group_into_text_rows_subscript_assigned_to_nearest() {
        // Normal chars at y=100 and y=120, subscript at y=102 (near first row)
        let chars = vec![
            CharInfo {
                ch: Some('P'),
                size: 10.0,
                origin_x: 0.0,
                origin_y: 100.0,
            },
            CharInfo {
                ch: Some('n'),
                size: 7.0,
                origin_x: 6.0,
                origin_y: 102.0,
            },
            CharInfo {
                ch: Some('Q'),
                size: 10.0,
                origin_x: 0.0,
                origin_y: 120.0,
            },
        ];
        let rows = group_into_text_rows(&chars, 10.0);
        assert_eq!(rows.len(), 2);
        // 'n' should be in the first row (closest to y=100)
        assert_eq!(rows[0].len(), 2);
        assert_eq!(rows[0][0].ch, Some('P'));
        assert_eq!(rows[0][1].ch, Some('n'));
        assert_eq!(rows[1].len(), 1);
        assert_eq!(rows[1][0].ch, Some('Q'));
    }

    #[test]
    fn test_group_into_text_rows_x_sorted() {
        // Characters in wrong x-order should be sorted
        let chars = vec![
            CharInfo {
                ch: Some('B'),
                size: 10.0,
                origin_x: 20.0,
                origin_y: 100.0,
            },
            CharInfo {
                ch: Some('A'),
                size: 10.0,
                origin_x: 0.0,
                origin_y: 100.0,
            },
        ];
        let rows = group_into_text_rows(&chars, 10.0);
        assert_eq!(rows[0][0].ch, Some('A'));
        assert_eq!(rows[0][1].ch, Some('B'));
    }

    #[test]
    fn test_build_line_interleaved_subscripts() {
        // Simulates the AISC pattern: φ(normal) c(sub) P(normal) n(sub)
        // sorted by x-position within a single row.
        let chars = vec![
            CharInfo {
                ch: Some('φ'),
                size: 9.5,
                origin_x: 205.0,
                origin_y: 372.0,
            },
            CharInfo {
                ch: Some('c'),
                size: 7.0,
                origin_x: 211.0,
                origin_y: 374.0,
            },
            CharInfo {
                ch: Some('P'),
                size: 9.5,
                origin_x: 215.0,
                origin_y: 372.0,
            },
            CharInfo {
                ch: Some('n'),
                size: 7.0,
                origin_x: 221.0,
                origin_y: 374.0,
            },
        ];
        let result = build_line_with_scripts_from_info(&chars, 9.5, 372.0);
        assert_eq!(result, "φ_{c}P_{n}");
    }
}
