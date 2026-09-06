use regex::Regex;
use serde::Serialize;
use std::cmp::Ordering;
use std::sync::OnceLock;

use crate::document::types::{Bbox, RawPage, RawWord};
use crate::formula::ocr::FormulaSidecarAttempt;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FormulaWord {
    pub text: String,
    pub bbox: Bbox,
    pub baseline_y: f32,
    pub font_size: f32,
}

impl From<&RawWord> for FormulaWord {
    fn from(word: &RawWord) -> Self {
        Self {
            text: word.text.clone(),
            bbox: word.bbox,
            baseline_y: word.baseline_y,
            font_size: word.font_size,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FormulaCandidate {
    pub page_num: usize,
    pub formula_index: usize,
    pub bbox: Bbox,
    pub source_text: String,
    /// Per-word geometry for geometric LaTeX recovery. Skipped in debug JSON to keep reports compact.
    #[serde(skip)]
    pub words: Vec<FormulaWord>,
    pub equation_number: Option<String>,
    pub confidence: u8,
    pub status: FormulaStatus,
    pub backend: Option<String>,
    pub latex: Option<String>,
    pub sidecar: FormulaSidecarAttempt,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FormulaStatus {
    LocalCandidate,
    NeedsReview,
    BackendRecovered,
}

#[derive(Clone, Debug)]
struct FormulaLine {
    bbox: Bbox,
    text: String,
    word_count: usize,
    math_score: usize,
    equation_number: Option<String>,
    words: Vec<RawWord>,
}

pub fn detect_formula_candidates(
    raw_page: &RawPage,
    excluded_bboxes: &[Bbox],
) -> Vec<FormulaCandidate> {
    if raw_page.words.is_empty() {
        return Vec::new();
    }

    let words = words_excluding_bboxes(&raw_page.words, excluded_bboxes);
    if words.is_empty() {
        return Vec::new();
    }

    let mut lines = group_words_into_lines(&words);

    if is_reference_section(&lines) {
        return Vec::new();
    }

    lines.sort_by(|a, b| {
        a.bbox
            .y0
            .partial_cmp(&b.bbox.y0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.bbox.x0.partial_cmp(&b.bbox.x0).unwrap_or(Ordering::Equal))
    });

    let page_bbox = raw_page.bbox();
    let mut candidates = Vec::new();
    for line in lines {
        if is_reference_line(&line.text) {
            continue;
        }
        let Some((confidence, reason)) = score_line(&line, raw_page.width) else {
            continue;
        };
        let padded = pad_and_clamp(line.bbox, 4.0, page_bbox);
        if excluded_bboxes.iter().any(|ex| {
            let inter_x0 = padded.x0.max(ex.x0);
            let inter_y0 = padded.y0.max(ex.y0);
            let inter_x1 = padded.x1.min(ex.x1);
            let inter_y1 = padded.y1.min(ex.y1);
            if inter_x1 <= inter_x0 || inter_y1 <= inter_y0 {
                return false;
            }
            let intersection = (inter_x1 - inter_x0) * (inter_y1 - inter_y0);
            let formula_area = (padded.x1 - padded.x0) * (padded.y1 - padded.y0);
            formula_area > 0.0 && intersection / formula_area > 0.5
        }) {
            continue;
        }
        let formula_words: Vec<FormulaWord> = line.words.iter().map(FormulaWord::from).collect();
        candidates.push(FormulaCandidate {
            page_num: raw_page.page_num,
            formula_index: candidates.len(),
            bbox: padded,
            source_text: line.text.trim().to_string(),
            words: formula_words,
            equation_number: line.equation_number,
            confidence,
            status: if confidence >= 70 {
                FormulaStatus::LocalCandidate
            } else {
                FormulaStatus::NeedsReview
            },
            backend: None,
            latex: None,
            sidecar: FormulaSidecarAttempt::not_attempted(),
            reason,
            crop_path: None,
        });
    }

    dedupe_nearby(candidates)
        .into_iter()
        .enumerate()
        .map(|(idx, mut candidate)| {
            candidate.formula_index = idx;
            candidate
        })
        .collect()
}

fn words_excluding_bboxes(words: &[RawWord], excluded_bboxes: &[Bbox]) -> Vec<RawWord> {
    if excluded_bboxes.is_empty() {
        return words.to_vec();
    }

    words
        .iter()
        .filter(|word| {
            !excluded_bboxes
                .iter()
                .any(|bbox| overlap_ratio(word.bbox, *bbox) > 0.50)
        })
        .cloned()
        .collect()
}

fn group_words_into_lines(words: &[RawWord]) -> Vec<FormulaLine> {
    let mut sorted: Vec<&RawWord> = words
        .iter()
        .filter(|word| !word.text.trim().is_empty())
        .collect();
    sorted.sort_by(|a, b| {
        a.baseline_y
            .partial_cmp(&b.baseline_y)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.bbox.x0.partial_cmp(&b.bbox.x0).unwrap_or(Ordering::Equal))
    });

    let mut groups: Vec<Vec<&RawWord>> = Vec::new();
    for word in sorted {
        let tolerance = (word.font_size * 0.65).clamp(3.0, 9.0);
        if let Some(group) = groups.last_mut() {
            let baseline = average_baseline(group);
            if (baseline - word.baseline_y).abs() <= tolerance {
                group.push(word);
                continue;
            }
        }
        groups.push(vec![word]);
    }

    groups
        .into_iter()
        .filter_map(|mut group| {
            group.sort_by(|a, b| a.bbox.x0.partial_cmp(&b.bbox.x0).unwrap_or(Ordering::Equal));
            let first = group.first()?;
            let mut bbox = first.bbox;
            let mut parts = Vec::new();
            for word in &group {
                bbox = bbox.union(&word.bbox);
                parts.push(word.text.trim());
            }
            let text = parts.join(" ");
            let raw_words: Vec<RawWord> = group.iter().map(|w| (*w).clone()).collect();
            Some(FormulaLine {
                bbox,
                word_count: group.len(),
                math_score: math_score(&text),
                equation_number: extract_equation_number(&text),
                text,
                words: raw_words,
            })
        })
        .collect()
}

fn average_baseline(words: &[&RawWord]) -> f32 {
    words.iter().map(|word| word.baseline_y).sum::<f32>() / words.len().max(1) as f32
}

fn score_line(line: &FormulaLine, page_width: f32) -> Option<(u8, String)> {
    let text = line.text.trim();
    if text.len() < 3 || text.len() > 180 || line.word_count > 28 {
        return None;
    }
    if looks_like_prose(text) {
        return None;
    }

    let centered = (line.bbox.center_x() - page_width / 2.0).abs() < page_width * 0.24;
    let has_equation_number = line.equation_number.is_some();
    let has_relation = text.contains('=')
        || text.contains('≤')
        || text.contains('≥')
        || text.contains('<')
        || text.contains('>');
    let symbol_heavy = line.math_score >= 2;
    let has_subscriptish =
        text.contains('_') || text.contains('₁') || text.contains('₂') || text.contains('^');

    if !(symbol_heavy || has_relation || has_subscriptish || has_equation_number) {
        return None;
    }
    if !centered && !has_equation_number && line.math_score < 3 {
        return None;
    }

    let mut confidence = 35u8;
    let mut reasons = Vec::new();
    if centered {
        confidence += 15;
        reasons.push("centered");
    }
    if has_equation_number {
        confidence += 22;
        reasons.push("equation-number");
    }
    if has_relation {
        confidence += 15;
        reasons.push("relation");
    }
    if centered && has_relation && line.word_count <= 4 {
        confidence += 10;
        reasons.push("compact-display");
    }
    if symbol_heavy {
        confidence += (line.math_score.min(5) as u8) * 4;
        reasons.push("math-symbols");
    }
    if has_subscriptish {
        confidence += 8;
        reasons.push("script-like");
    }

    Some((confidence.min(96), reasons.join("+")))
}

fn looks_like_prose(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("such as")
        || lower.contains("typeset")
        || lower.contains("detailed working")
        || lower.contains("not reproduced")
    {
        return true;
    }

    let words = text.split_whitespace().count();
    if words < 10 {
        return false;
    }
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let math = math_score(text);
    letters > 45 && math < 2 && !text.contains('=')
}

fn extract_equation_number(text: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)(?:^|\s)(\((?:p\.)?\d+(?:[.-]\d+)*\)|\(\d+(?:\.\d+){1,4}\))\s*$")
            .expect("equation number regex")
    });
    re.captures(text)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().trim().to_string())
}

fn math_score(text: &str) -> usize {
    text.chars()
        .filter(|c| {
            matches!(
                *c,
                '=' | '+'
                    | '−'
                    | '-'
                    | '×'
                    | '*'
                    | '/'
                    | '÷'
                    | '<'
                    | '>'
                    | '≤'
                    | '≥'
                    | '√'
                    | '∑'
                    | '∫'
                    | '∂'
                    | '∆'
                    | 'Δ'
                    | 'π'
                    | 'μ'
                    | 'σ'
                    | 'τ'
                    | 'γ'
                    | 'α'
                    | 'β'
                    | 'θ'
                    | 'λ'
                    | 'φ'
                    | 'Ω'
            )
        })
        .count()
}

fn pad_and_clamp(bbox: Bbox, padding: f32, page_bbox: Bbox) -> Bbox {
    Bbox::new(
        (bbox.x0 - padding).max(page_bbox.x0),
        (bbox.y0 - padding).max(page_bbox.y0),
        (bbox.x1 + padding).min(page_bbox.x1),
        (bbox.y1 + padding).min(page_bbox.y1),
    )
}

fn dedupe_nearby(candidates: Vec<FormulaCandidate>) -> Vec<FormulaCandidate> {
    let mut kept: Vec<FormulaCandidate> = Vec::new();
    for candidate in candidates {
        if kept
            .iter()
            .any(|existing| overlap_ratio(existing.bbox, candidate.bbox) > 0.70)
        {
            continue;
        }
        kept.push(candidate);
    }
    kept
}

fn overlap_ratio(a: Bbox, b: Bbox) -> f32 {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    let intersection = ((x1 - x0).max(0.0)) * ((y1 - y0).max(0.0));
    intersection / a.area().min(b.area()).max(1.0)
}

/// Returns true when the line looks like a bibliography or standards reference entry.
/// Patterns matched:
///   `/N/`  — DNV-style (e.g. `/34/ DNV-RU-OU-0300`)
///   `[N]`  — IEEE/academic bracketed ref
///   `(N)`  — numbered note
/// N must be 1–4 ASCII digits.
fn is_reference_line(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    // /N/ pattern
    if let Some(rest) = t.strip_prefix('/') {
        if let Some(slash) = rest.find('/') {
            let maybe_num = &rest[..slash];
            if !maybe_num.is_empty()
                && maybe_num.len() <= 4
                && maybe_num.chars().all(|c| c.is_ascii_digit())
            {
                return true;
            }
        }
    }
    // [N] or (N) pattern — only matches when followed by more content.
    // "(3.1)" alone is an equation label; "[1] Author..." is a reference.
    let first = t.chars().next().unwrap_or(' ');
    if first == '[' || first == '(' {
        let close = if first == '[' { ']' } else { ')' };
        if let Some(i) = t.find(close) {
            let inner = &t[1..i];
            let after = t[i + 1..].trim(); // text after the closing bracket
            if !inner.is_empty()
                && inner.len() <= 4
                && inner.chars().all(|c| c.is_ascii_digit())
                && !after.is_empty()
            // require following content
            {
                return true;
            }
        }
    }
    false
}

/// Returns true if this page looks like a reference/bibliography section.
/// Two signals: any spatially-grouped line whose full text is a reference
/// heading, OR >40% of lines start with a reference marker.
/// Accepts pre-computed lines to avoid a redundant `group_words_into_lines` call.
fn is_reference_section(lines: &[FormulaLine]) -> bool {
    if lines.is_empty() {
        return false;
    }
    // Fast path: any line whose full text is a reference heading.
    for line in lines {
        let t = line.text.trim();
        if matches!(
            t,
            "References" | "Bibliography" | "REFERENCES" | "BIBLIOGRAPHY"
        ) {
            return true;
        }
    }
    // Density path: >40% of lines start with a reference marker.
    let ref_count = lines.iter().filter(|l| is_reference_line(&l.text)).count();
    ref_count * 10 >= lines.len() * 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::types::{RawPage, RawWord};

    fn word(text: &str, x0: f32, y0: f32, x1: f32, y1: f32, line_id: usize) -> RawWord {
        RawWord {
            bbox: Bbox::new(x0, y0, x1, y1),
            text: text.to_string(),
            font_size: 10.0,
            page_num: 0,
            block_id: 0,
            line_id,
            baseline_y: y1,
        }
    }

    fn page(words: Vec<RawWord>) -> RawPage {
        RawPage {
            page_num: 0,
            width: 600.0,
            height: 800.0,
            blocks: Vec::new(),
            words,
            image_refs: Vec::new(),
        }
    }

    #[test]
    fn detects_centered_equation_with_number() {
        let raw = page(vec![
            word("F", 210.0, 100.0, 222.0, 112.0, 0),
            word("=", 230.0, 100.0, 238.0, 112.0, 0),
            word("m", 248.0, 100.0, 260.0, 112.0, 0),
            word("×", 268.0, 100.0, 276.0, 112.0, 0),
            word("a", 286.0, 100.0, 298.0, 112.0, 0),
            word("(1)", 430.0, 100.0, 450.0, 112.0, 0),
        ]);

        let candidates = detect_formula_candidates(&raw, &[]);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].equation_number.as_deref(), Some("(1)"));
        assert!(candidates[0].confidence >= 70);
    }

    #[test]
    fn ignores_normal_paragraph_with_parentheses() {
        let raw = page(vec![
            word("This", 40.0, 100.0, 62.0, 112.0, 0),
            word("paragraph", 66.0, 100.0, 118.0, 112.0, 0),
            word("mentions", 122.0, 100.0, 170.0, 112.0, 0),
            word("(1)", 174.0, 100.0, 190.0, 112.0, 0),
            word("without", 194.0, 100.0, 238.0, 112.0, 0),
            word("being", 242.0, 100.0, 272.0, 112.0, 0),
            word("a", 276.0, 100.0, 282.0, 112.0, 0),
            word("formula", 286.0, 100.0, 330.0, 112.0, 0),
            word("line.", 334.0, 100.0, 360.0, 112.0, 0),
        ]);

        assert!(detect_formula_candidates(&raw, &[]).is_empty());
    }

    #[test]
    fn keeps_formula_candidate_bbox_inside_page() {
        let raw = page(vec![
            word("σ", 1.0, 2.0, 8.0, 14.0, 0),
            word("=", 12.0, 2.0, 20.0, 14.0, 0),
            word("F/A", 24.0, 2.0, 48.0, 14.0, 0),
            word("(P.3-2)", 52.0, 2.0, 92.0, 14.0, 0),
        ]);

        let candidates = detect_formula_candidates(&raw, &[]);

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].bbox.x0 >= 0.0);
        assert!(candidates[0].bbox.y0 >= 0.0);
    }
}
