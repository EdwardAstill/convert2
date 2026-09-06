//! Detect candidate table regions from rule lines and word alignment.

use crate::document::types::{Bbox, RawWord};
use crate::layout::drawing_ops::{HLine, VLine};
use crate::layout::table::{TableEvidence, TableEvidenceSource};
use std::collections::HashSet;

const MIN_LINE_TABLE_WIDTH_FRACTION: f32 = 0.30;
const MIN_LINE_BAND_HEIGHT: f32 = 8.0;
const MAX_LINE_BAND_HEIGHT: f32 = 320.0;
const LINE_TEXT_PADDING: f32 = 4.0;
const MIN_LINE_BAND_WORDS: usize = 3;
const MIN_WS_COLUMNS: usize = 3;
const MIN_WS_ROWS: usize = 3;
const ROW_TOLERANCE_PT: f32 = 8.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GeometryTableRegion {
    pub bbox: Bbox,
    pub rows: Vec<Vec<String>>,
    pub layout_text: String,
    pub source_block_ids: std::collections::BTreeSet<usize>,
    pub row_consistency: f32,
    pub confidence: f32,
    pub evidence: TableEvidence,
}

#[derive(Clone, Debug, PartialEq)]
struct GeometryRegion {
    bbox: Bbox,
    source: TableEvidenceSource,
    ruling_intersections: usize,
}

/// Returns bounding boxes of candidate table regions.
pub fn detect_table_regions(
    hlines: &[HLine],
    vlines: &[VLine],
    words: &[RawWord],
    page_width: f32,
    page_height: f32,
) -> Vec<Bbox> {
    detect_table_region_candidates(hlines, vlines, words, page_width, page_height)
        .into_iter()
        .map(|candidate| candidate.bbox)
        .collect()
}

pub(crate) fn detect_table_region_candidates(
    hlines: &[HLine],
    vlines: &[VLine],
    words: &[RawWord],
    page_width: f32,
    page_height: f32,
) -> Vec<GeometryTableRegion> {
    let mut regions = line_pair_regions(hlines, words, page_width, page_height);
    regions.extend(grid_regions(hlines, vlines, words, page_width, page_height));
    if let Some(region) = whitespace_table_region(words, page_width) {
        regions.push(region);
    }
    merge_regions(regions)
        .into_iter()
        .filter_map(|region| geometry_region_candidate(region, words, page_height))
        .collect()
}

fn line_pair_regions(
    hlines: &[HLine],
    words: &[RawWord],
    page_width: f32,
    page_height: f32,
) -> Vec<GeometryRegion> {
    let mut significant: Vec<&HLine> = hlines
        .iter()
        .filter(|line| {
            line.is_significant() && line.length() >= page_width * MIN_LINE_TABLE_WIDTH_FRACTION
        })
        .collect();
    significant.sort_by(|left, right| {
        left.y
            .partial_cmp(&right.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut regions = Vec::new();
    for pair in significant.windows(2) {
        let top = pair[0];
        let bottom = pair[1];
        let height = bottom.y - top.y;
        if !(MIN_LINE_BAND_HEIGHT..=MAX_LINE_BAND_HEIGHT).contains(&height) {
            continue;
        }
        let x0 = top.x0.min(bottom.x0);
        let x1 = top.x1.max(bottom.x1);
        let words_in_band: Vec<&RawWord> = words
            .iter()
            .filter(|word| word_between_rules(word, top.y, bottom.y, x0, x1))
            .collect();
        if words_in_band.len() < MIN_LINE_BAND_WORDS {
            continue;
        }
        if count_rows(&words_in_band) < 1 || count_columns(&words_in_band, page_width) < 2 {
            continue;
        }
        regions.push(GeometryRegion {
            bbox: Bbox::new(
                x0.max(0.0),
                top.y.max(0.0),
                x1.min(page_width),
                bottom.y.min(page_height),
            ),
            source: TableEvidenceSource::RulingBand,
            ruling_intersections: 0,
        });
    }
    regions
}

fn grid_regions(
    hlines: &[HLine],
    vlines: &[VLine],
    words: &[RawWord],
    page_width: f32,
    page_height: f32,
) -> Vec<GeometryRegion> {
    let significant_h: Vec<&HLine> = hlines.iter().filter(|line| line.is_significant()).collect();
    let significant_v: Vec<&VLine> = vlines.iter().filter(|line| line.is_significant()).collect();
    if significant_h.len() < 2 || significant_v.len() < 2 {
        return Vec::new();
    }

    let x0 = significant_v
        .iter()
        .map(|line| line.x)
        .fold(f32::INFINITY, f32::min);
    let x1 = significant_v
        .iter()
        .map(|line| line.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let y0 = significant_h
        .iter()
        .map(|line| line.y)
        .fold(f32::INFINITY, f32::min);
    let y1 = significant_h
        .iter()
        .map(|line| line.y)
        .fold(f32::NEG_INFINITY, f32::max);

    if x1 - x0 < page_width * MIN_LINE_TABLE_WIDTH_FRACTION
        || y1 <= y0
        || y1 - y0 > MAX_LINE_BAND_HEIGHT * 2.0
    {
        return Vec::new();
    }
    let bbox = Bbox::new(
        x0.clamp(0.0, page_width),
        y0.clamp(0.0, page_height),
        x1.clamp(0.0, page_width),
        y1.clamp(0.0, page_height),
    );
    let words_in_grid = words
        .iter()
        .filter(|word| bbox.overlaps(&word.bbox))
        .count();
    if words_in_grid < MIN_LINE_BAND_WORDS {
        return Vec::new();
    }
    vec![GeometryRegion {
        bbox,
        source: TableEvidenceSource::RulingGrid,
        ruling_intersections: significant_h.len() * significant_v.len(),
    }]
}

fn whitespace_table_region(words: &[RawWord], page_width: f32) -> Option<GeometryRegion> {
    if words.len() < MIN_WS_COLUMNS * MIN_WS_ROWS {
        return None;
    }

    let mut rows = word_rows(words);
    rows.retain(|row| row.len() >= MIN_WS_COLUMNS);
    if rows.len() < MIN_WS_ROWS {
        return None;
    }

    let column_centres = stable_column_centres(&rows, page_width)?;
    if column_centres.len() < MIN_WS_COLUMNS {
        return None;
    }

    let qualifying_rows: Vec<Vec<&RawWord>> = rows
        .into_iter()
        .filter(|row| row_hits_columns(row, &column_centres) >= MIN_WS_COLUMNS)
        .collect();
    if qualifying_rows.len() < MIN_WS_ROWS {
        return None;
    }

    let all_words: Vec<&RawWord> = qualifying_rows.into_iter().flatten().collect();
    Some(GeometryRegion {
        bbox: words_bbox(&all_words)?.pad(3.0, 3.0),
        source: TableEvidenceSource::TextAlignment,
        ruling_intersections: 0,
    })
}

fn word_between_rules(word: &RawWord, top: f32, bottom: f32, x0: f32, x1: f32) -> bool {
    word.bbox.y0 >= top - LINE_TEXT_PADDING
        && word.bbox.y1 <= bottom + LINE_TEXT_PADDING
        && word.bbox.center_x() >= x0 - LINE_TEXT_PADDING
        && word.bbox.center_x() <= x1 + LINE_TEXT_PADDING
}

fn word_rows(words: &[RawWord]) -> Vec<Vec<&RawWord>> {
    let mut sorted: Vec<&RawWord> = words
        .iter()
        .filter(|word| !word.text.trim().is_empty())
        .collect();
    sorted.sort_by(|left, right| {
        left.bbox
            .center_y()
            .partial_cmp(&right.bbox.center_y())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<Vec<&RawWord>> = Vec::new();
    for word in sorted {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| (row_center_y(row) - word.bbox.center_y()).abs() <= ROW_TOLERANCE_PT)
        {
            row.push(word);
        } else {
            rows.push(vec![word]);
        }
    }
    for row in &mut rows {
        row.sort_by(|left, right| {
            left.bbox
                .center_x()
                .partial_cmp(&right.bbox.center_x())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    rows
}

fn stable_column_centres(rows: &[Vec<&RawWord>], page_width: f32) -> Option<Vec<f32>> {
    let mut centres: Vec<f32> = rows
        .iter()
        .flat_map(|row| row.iter().map(|word| word.bbox.center_x()))
        .collect();
    centres.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));

    let tolerance = (page_width * 0.018).clamp(8.0, 18.0);
    let mut clusters: Vec<Vec<f32>> = Vec::new();
    for centre in centres {
        if let Some(cluster) = clusters.last_mut() {
            let cluster_mean = cluster.iter().sum::<f32>() / cluster.len() as f32;
            if (centre - cluster_mean).abs() <= tolerance {
                cluster.push(centre);
                continue;
            }
        }
        clusters.push(vec![centre]);
    }

    let min_hits = rows.len().min(MIN_WS_ROWS);
    let columns: Vec<f32> = clusters
        .into_iter()
        .filter(|cluster| cluster.len() >= min_hits)
        .map(|cluster| cluster.iter().sum::<f32>() / cluster.len() as f32)
        .collect();
    (!columns.is_empty()).then_some(columns)
}

fn row_hits_columns(row: &[&RawWord], column_centres: &[f32]) -> usize {
    let mut hits = HashSet::new();
    for word in row {
        if let Some((idx, distance)) = column_centres
            .iter()
            .enumerate()
            .map(|(idx, centre)| (idx, (word.bbox.center_x() - centre).abs()))
            .min_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            if distance <= word.font_size.max(8.0) * 2.0 {
                hits.insert(idx);
            }
        }
    }
    hits.len()
}

fn count_rows(words: &[&RawWord]) -> usize {
    word_rows_from_refs(words).len()
}

fn count_columns(words: &[&RawWord], page_width: f32) -> usize {
    let rows = word_rows_from_refs(words);
    stable_column_centres(&rows, page_width).map_or(0, |columns| columns.len())
}

fn word_rows_from_refs<'a>(words: &[&'a RawWord]) -> Vec<Vec<&'a RawWord>> {
    let owned: Vec<RawWord> = words.iter().map(|word| (*word).clone()).collect();
    let rows = word_rows(&owned);
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .filter_map(|owned_word| {
                    words
                        .iter()
                        .copied()
                        .find(|word| word.bbox == owned_word.bbox && word.text == owned_word.text)
                })
                .collect()
        })
        .collect()
}

fn row_center_y(row: &[&RawWord]) -> f32 {
    row.iter().map(|word| word.bbox.center_y()).sum::<f32>() / row.len().max(1) as f32
}

fn words_bbox(words: &[&RawWord]) -> Option<Bbox> {
    words
        .iter()
        .map(|word| word.bbox)
        .reduce(|left, right| left.union(&right))
}

fn geometry_region_candidate(
    region: GeometryRegion,
    words: &[RawWord],
    page_height: f32,
) -> Option<GeometryTableRegion> {
    let bbox = region.bbox;
    let mut region_words: Vec<&RawWord> = words
        .iter()
        .filter(|word| bbox_overlap_ratio(word.bbox, bbox) > 0.20 || bbox.overlaps(&word.bbox))
        .collect();
    if region_words.len() < MIN_LINE_BAND_WORDS {
        return None;
    }
    region_words.sort_by(|left, right| {
        left.baseline_y
            .partial_cmp(&right.baseline_y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.bbox
                    .x0
                    .partial_cmp(&right.bbox.x0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let rows = geometry_words_to_rows(&region_words);
    if rows.is_empty() {
        return None;
    }
    if prose_page_like_geometry_region(&rows, bbox, page_height) {
        return None;
    }
    let layout_text = geometry_rows_to_layout_text(&rows);
    let markdown_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.iter().map(|word| word.text.clone()).collect())
        .collect();
    let source_block_ids = region_words.iter().map(|word| word.block_id).collect();
    let row_consistency = row_width_consistency(&markdown_rows);
    let evidence = geometry_table_evidence(
        &rows,
        row_consistency,
        bbox,
        page_height,
        region.source,
        region.ruling_intersections,
    );
    let confidence = evidence.score();

    Some(GeometryTableRegion {
        bbox,
        rows: markdown_rows,
        layout_text,
        source_block_ids,
        row_consistency,
        confidence,
        evidence,
    })
}

fn prose_page_like_geometry_region(rows: &[Vec<&RawWord>], region: Bbox, page_height: f32) -> bool {
    if rows.len() < 8 || region.height() < page_height.max(1.0) * 0.45 {
        return false;
    }
    let prose_rows = rows
        .iter()
        .filter(|row| geometry_row_looks_like_prose(row))
        .count();
    prose_rows * 10 >= rows.len() * 3
}

fn geometry_row_looks_like_prose(row: &[&RawWord]) -> bool {
    if row.len() < 6 {
        return false;
    }
    let stopwords = [
        "a", "an", "and", "are", "as", "at", "for", "from", "in", "is", "of", "on", "or", "the",
        "to", "with", "without",
    ];
    let stopword_count = row
        .iter()
        .filter(|word| stopwords.contains(&word.text.to_ascii_lowercase().as_str()))
        .count();
    let digit_rows = row
        .iter()
        .filter(|word| word.text.chars().any(|ch| ch.is_ascii_digit()))
        .count();
    stopword_count >= 1 && digit_rows == 0
}

fn geometry_table_evidence(
    rows: &[Vec<&RawWord>],
    row_consistency: f32,
    bbox: Bbox,
    page_height: f32,
    source: TableEvidenceSource,
    ruling_intersections: usize,
) -> TableEvidence {
    let word_count = rows.iter().map(Vec::len).sum::<usize>().max(1);
    let numeric_density = rows
        .iter()
        .flat_map(|row| row.iter())
        .filter(|word| geometry_word_looks_like_table_value(&word.text))
        .count() as f32
        / word_count as f32;
    let prose_ratio = rows
        .iter()
        .filter(|row| geometry_row_looks_like_prose(row))
        .count() as f32
        / rows.len().max(1) as f32;
    let broad_page_penalty = if bbox.height() >= page_height.max(1.0) * 0.35 {
        match source {
            TableEvidenceSource::TextAlignment => 0.28,
            TableEvidenceSource::RulingBand => 0.12,
            TableEvidenceSource::RulingGrid => 0.04,
            _ => 0.10,
        }
    } else {
        0.0
    };
    let column_alignment = (row_consistency + numeric_density.max(0.20)) / 2.0;
    TableEvidence {
        source,
        row_consistency,
        column_alignment: column_alignment.clamp(0.0, 1.0),
        numeric_density,
        row_count: rows.len(),
        ruling_intersections,
        caption_score: 0.0,
        broad_page_penalty,
        prose_penalty: prose_ratio * 0.35,
        debug_reasons: vec![format!("{source:?}")],
    }
}

fn geometry_word_looks_like_table_value(text: &str) -> bool {
    let trimmed = text.trim_matches(|ch: char| matches!(ch, ',' | ';' | ':' | '†' | '‡' | '*'));
    if trimmed.is_empty() {
        return false;
    }
    let digit_count = trimmed.chars().filter(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return matches!(trimmed, "-" | "–" | "—");
    }
    let content_len = trimmed.chars().filter(|ch| !ch.is_whitespace()).count();
    digit_count * 2 >= content_len.max(1)
}

fn geometry_words_to_rows<'a>(words: &[&'a RawWord]) -> Vec<Vec<&'a RawWord>> {
    let mut rows: Vec<Vec<&RawWord>> = Vec::new();
    for word in words {
        let tolerance = (word.font_size * 0.65).max(4.0);
        if let Some(row) = rows
            .iter_mut()
            .find(|row| (row_baseline(row) - word.baseline_y).abs() <= tolerance)
        {
            row.push(*word);
        } else {
            rows.push(vec![*word]);
        }
    }
    for row in &mut rows {
        row.sort_by(|left, right| {
            left.bbox
                .x0
                .partial_cmp(&right.bbox.x0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    rows
}

fn row_baseline(words: &[&RawWord]) -> f32 {
    words.iter().map(|word| word.baseline_y).sum::<f32>() / words.len().max(1) as f32
}

fn geometry_rows_to_layout_text(rows: &[Vec<&RawWord>]) -> String {
    let min_x = rows
        .iter()
        .flat_map(|row| row.iter().map(|word| word.bbox.x0))
        .fold(f32::INFINITY, f32::min);
    let avg_font_size = rows
        .iter()
        .flat_map(|row| row.iter().map(|word| word.font_size))
        .sum::<f32>()
        / rows.iter().map(Vec::len).sum::<usize>().max(1) as f32;
    let points_per_char = (avg_font_size * 0.45).max(3.5);

    rows.iter()
        .map(|row| {
            let mut line = String::new();
            for word in row {
                let target = ((word.bbox.x0 - min_x) / points_per_char).round().max(0.0) as usize;
                if line.len() < target {
                    line.push_str(&" ".repeat(target - line.len()));
                } else if !line.is_empty() && !line.ends_with(' ') {
                    line.push(' ');
                }
                line.push_str(word.text.trim());
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn row_width_consistency(rows: &[Vec<String>]) -> f32 {
    let mut counts = std::collections::BTreeMap::new();
    for row in rows {
        *counts.entry(row.len()).or_insert(0usize) += 1;
    }
    counts
        .values()
        .copied()
        .max()
        .map(|max| max as f32 / rows.len().max(1) as f32)
        .unwrap_or(0.0)
}

fn merge_regions(regions: Vec<GeometryRegion>) -> Vec<GeometryRegion> {
    let mut kept: Vec<GeometryRegion> = Vec::new();
    'candidate: for region in regions {
        if region.bbox.width() <= 0.0 || region.bbox.height() <= 0.0 {
            continue;
        }
        for existing in &mut kept {
            if overlap_ratio(region.bbox, existing.bbox) > 0.60 {
                existing.bbox = existing.bbox.union(&region.bbox);
                if geometry_source_rank(region.source) > geometry_source_rank(existing.source) {
                    existing.source = region.source;
                }
                existing.ruling_intersections = existing
                    .ruling_intersections
                    .max(region.ruling_intersections);
                continue 'candidate;
            }
        }
        kept.push(region);
    }
    kept
}

fn geometry_source_rank(source: TableEvidenceSource) -> u8 {
    match source {
        TableEvidenceSource::RulingGrid => 4,
        TableEvidenceSource::RulingBand => 3,
        TableEvidenceSource::NumericRows => 3,
        TableEvidenceSource::ExplicitRegion | TableEvidenceSource::ExternalModel => 3,
        TableEvidenceSource::TextAlignment => 1,
    }
}

fn overlap_ratio(a: Bbox, b: Bbox) -> f32 {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    let intersection = ((x1 - x0).max(0.0)) * ((y1 - y0).max(0.0));
    intersection / a.area().min(b.area()).max(1.0)
}

fn bbox_overlap_ratio(a: Bbox, b: Bbox) -> f32 {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    let intersection = ((x1 - x0).max(0.0)) * ((y1 - y0).max(0.0));
    intersection / a.area().max(1.0)
}

trait BboxPad {
    fn pad(self, x: f32, y: f32) -> Self;
}

impl BboxPad for Bbox {
    fn pad(self, x: f32, y: f32) -> Self {
        Bbox::new(self.x0 - x, self.y0 - y, self.x1 + x, self.y1 + y)
    }
}
