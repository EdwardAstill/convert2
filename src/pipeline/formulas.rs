//! Formula stage of the conversion pipeline: sidecar construction, candidate
//! reporting and emission decisions, LaTeX recovery, and debug/audit output.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;

use crate::config::{self, ConvertOptions, FormulaEmitMode, FormulaMode};
use crate::document::types::{Block, BlockKind};
use crate::formula::detect::FormulaStatus;
use crate::formula::geometric::geometric_latex;
use crate::formula::ocr::{
    FormulaSidecar, FormulaSidecarAttempt, FormulaSidecarStatus, SubprocessSidecar,
};
#[cfg(feature = "onnx-ocr")]
use crate::formula::ocr_onnx::OnnxFormulaSidecar;
use crate::formula::FormulaCandidate;

pub(super) fn build_formula_sidecar(
    value: Option<&str>,
    timeout: Duration,
) -> anyhow::Result<Option<Box<dyn FormulaSidecar>>> {
    let Some(value) = value else {
        return Ok(None);
    };

    match config::parse_formula_sidecar(value)? {
        config::FormulaSidecarArg::Command(command) => Ok(Some(Box::new(
            SubprocessSidecar::with_timeout(command, timeout),
        ))),
        #[cfg(feature = "onnx-ocr")]
        config::FormulaSidecarArg::Onnx(model_dir) => {
            let sidecar = OnnxFormulaSidecar::new(&model_dir).with_context(|| {
                format!(
                    "failed to initialise ONNX formula sidecar from {}",
                    model_dir.display()
                )
            })?;
            Ok(Some(Box::new(sidecar)))
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct FormulaDebugIndex<'a> {
    schema_version: u8,
    source_pdf: String,
    page_count: usize,
    candidate_count: usize,
    pages_with_candidates: usize,
    local_candidate_count: usize,
    needs_review_count: usize,
    backend_recovered_count: usize,
    emitted_count: usize,
    review_block_count: usize,
    candidates: &'a [FormulaReportRecord],
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct FormulaReportRecord {
    page: usize,
    formula_index: usize,
    confidence: u8,
    status: FormulaStatus,
    backend: Option<String>,
    emitted: bool,
    review_block: bool,
    equation_number: Option<String>,
    crop_path: Option<String>,
    source_text: String,
    latex: Option<String>,
    sidecar: FormulaSidecarAttempt,
    sanity: Option<String>,
    emission_reason: String,
    reason: String,
}

pub(super) fn formula_report_records(
    candidates: &[FormulaCandidate],
    mode: config::FormulaMode,
    render_math: bool,
    emit_mode: FormulaEmitMode,
) -> Vec<FormulaReportRecord> {
    candidates
        .iter()
        .map(|candidate| {
            let review_block = is_unresolved_formula_review(candidate);
            let decision = formula_emission_decision(candidate, mode, render_math, emit_mode);
            let emitted = !review_block && decision.emit;
            let latex = if emitted {
                Some(build_formula_latex(candidate))
            } else {
                candidate.latex.clone()
            };

            let sanity = if candidate.latex.is_some()
                && matches!(candidate.status, FormulaStatus::BackendRecovered)
            {
                Some("passed".to_string())
            } else {
                candidate.sidecar.sanity.clone()
            };

            FormulaReportRecord {
                page: candidate.page_num + 1,
                formula_index: candidate.formula_index + 1,
                confidence: candidate.confidence,
                status: candidate.status.clone(),
                backend: candidate.backend.clone(),
                emitted,
                review_block,
                equation_number: candidate.equation_number.clone(),
                crop_path: candidate.crop_path.clone(),
                source_text: candidate.source_text.clone(),
                latex,
                sidecar: candidate.sidecar.clone(),
                sanity,
                emission_reason: if review_block {
                    "review-block".to_string()
                } else {
                    decision.reason
                },
                reason: candidate.reason.clone(),
            }
        })
        .collect()
}

pub(super) fn formula_candidates_to_blocks(
    page_num: usize,
    candidates: Vec<FormulaCandidate>,
    mode: config::FormulaMode,
    render_math: bool,
    emit_mode: FormulaEmitMode,
) -> Vec<Block> {
    if matches!(mode, config::FormulaMode::Off) {
        return Vec::new();
    }

    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(idx, candidate)| {
            if is_unresolved_formula_review(&candidate) {
                return Some(Block::special(
                    3_100_000 + idx,
                    candidate.bbox,
                    BlockKind::FormulaReview {
                        reason: candidate.reason,
                        crop_path: candidate.crop_path,
                    },
                    page_num,
                    0.0,
                    "formula-review".to_string(),
                ));
            }

            if !formula_emission_decision(&candidate, mode, render_math, emit_mode).emit {
                return None;
            }

            let latex = build_formula_latex(&candidate);
            Some(Block {
                override_markdown: None,
                id: 3_000_000 + idx,
                bbox: candidate.bbox,
                text: candidate.source_text,
                kind: BlockKind::Formula {
                    latex,
                    display: true,
                },
                font_size: 0.0,
                font_name: "formula-candidate".to_string(),
                page_num,
                reading_order: 0,
                bold: false,
                italic: false,
            })
        })
        .collect()
}

pub(super) fn is_unresolved_formula_review(candidate: &FormulaCandidate) -> bool {
    candidate.latex.is_none()
        && candidate.source_text.trim().is_empty()
        && candidate.backend.as_deref() == Some("visual-page-render")
}

#[derive(Debug, Clone)]
pub(super) struct FormulaEmissionDecision {
    emit: bool,
    reason: String,
}

pub(super) fn formula_emission_decision(
    candidate: &FormulaCandidate,
    mode: config::FormulaMode,
    render_math: bool,
    emit_mode: FormulaEmitMode,
) -> FormulaEmissionDecision {
    if !render_math {
        return reject_formula("render-math-disabled");
    }
    if matches!(mode, config::FormulaMode::Off) || matches!(emit_mode, FormulaEmitMode::None) {
        return reject_formula("formula-emission-disabled");
    }
    if candidate.latex.is_none() && candidate.source_text.trim().is_empty() {
        return reject_formula("empty-formula-text");
    }
    if has_replacement_or_gibberish_markers(candidate) {
        return reject_formula("unsafe-source-text");
    }
    if candidate.latex.is_some() {
        return emit_formula("sidecar-recovered");
    }

    match emit_mode {
        FormulaEmitMode::None => reject_formula("formula-emission-disabled"),
        FormulaEmitMode::Conservative => {
            reject_formula("local-heuristic-rejected-by-conservative-policy")
        }
        FormulaEmitMode::Auto => match mode {
            config::FormulaMode::Auto if candidate.confidence >= 70 => {
                emit_formula("high-confidence-local-auto")
            }
            config::FormulaMode::Local | config::FormulaMode::Hybrid => emit_formula("local-mode"),
            config::FormulaMode::Auto => reject_formula("low-confidence-local-auto"),
            config::FormulaMode::Off => reject_formula("formula-mode-off"),
        },
        FormulaEmitMode::All => emit_formula("formula-emit-all"),
    }
}

pub(super) fn emit_formula(reason: &str) -> FormulaEmissionDecision {
    FormulaEmissionDecision {
        emit: true,
        reason: reason.to_string(),
    }
}

pub(super) fn reject_formula(reason: &str) -> FormulaEmissionDecision {
    FormulaEmissionDecision {
        emit: false,
        reason: reason.to_string(),
    }
}

pub(super) fn has_replacement_or_gibberish_markers(candidate: &FormulaCandidate) -> bool {
    let text = candidate.source_text.trim();
    text.contains('�')
        || text.matches('?').count() >= 3
        || looks_like_malformed_matrix_fragment(text)
}

pub(super) fn looks_like_malformed_matrix_fragment(text: &str) -> bool {
    let open = text.matches('(').count();
    let close = text.matches(')').count();
    let whitespace_runs = text.split_whitespace().count();
    open >= 2 && close >= 2 && whitespace_runs >= 5 && !text.contains('[') && !text.contains('\\')
}

pub(super) fn renumber_formula_candidates(candidates: &mut [FormulaCandidate]) {
    for (idx, candidate) in candidates.iter_mut().enumerate() {
        candidate.formula_index = idx;
    }
}

pub(super) fn build_formula_latex(candidate: &FormulaCandidate) -> String {
    candidate.latex.clone().unwrap_or_else(|| {
        let mut text = candidate.source_text.clone();
        if let Some(eq_num) = &candidate.equation_number {
            if let Some(stripped) = text.trim_end().strip_suffix(eq_num.as_str()) {
                let tag_inner = eq_num.trim_matches(|c| c == '(' || c == ')');
                text = format!("{} \\tag{{{}}}", stripped.trim_end(), tag_inner);
            }
        }
        if !candidate.words.is_empty() {
            geometric_latex(&candidate.words, &text, unicode_to_latex)
        } else {
            unicode_to_latex(&text)
        }
    })
}

pub(super) fn unicode_to_latex(s: &str) -> String {
    s.chars()
        .fold(String::with_capacity(s.len() + 16), |mut out, c| {
            match c {
                'α' => out.push_str("\\alpha "),
                'β' => out.push_str("\\beta "),
                'γ' => out.push_str("\\gamma "),
                'δ' => out.push_str("\\delta "),
                'ε' => out.push_str("\\varepsilon "),
                'ζ' => out.push_str("\\zeta "),
                'η' => out.push_str("\\eta "),
                'θ' => out.push_str("\\theta "),
                'λ' => out.push_str("\\lambda "),
                'μ' => out.push_str("\\mu "),
                'ν' => out.push_str("\\nu "),
                'ξ' => out.push_str("\\xi "),
                'π' => out.push_str("\\pi "),
                'ρ' => out.push_str("\\rho "),
                'σ' => out.push_str("\\sigma "),
                'τ' => out.push_str("\\tau "),
                'φ' => out.push_str("\\phi "),
                'χ' => out.push_str("\\chi "),
                'ψ' => out.push_str("\\psi "),
                'ω' => out.push_str("\\omega "),
                'Γ' => out.push_str("\\Gamma "),
                'Δ' | '∆' => out.push_str("\\Delta "),
                'Θ' => out.push_str("\\Theta "),
                'Λ' => out.push_str("\\Lambda "),
                'Π' => out.push_str("\\Pi "),
                'Σ' => out.push_str("\\Sigma "),
                'Φ' => out.push_str("\\Phi "),
                'Ψ' => out.push_str("\\Psi "),
                'Ω' => out.push_str("\\Omega "),
                '∑' => out.push_str("\\sum "),
                '∏' => out.push_str("\\prod "),
                '∫' => out.push_str("\\int "),
                '∂' => out.push_str("\\partial "),
                '∞' => out.push_str("\\infty "),
                '√' => out.push_str("\\sqrt{} "),
                '±' => out.push_str("\\pm "),
                '∓' => out.push_str("\\mp "),
                '×' => out.push_str("\\times "),
                '÷' => out.push_str("\\div "),
                '≤' => out.push_str("\\leq "),
                '≥' => out.push_str("\\geq "),
                '≠' => out.push_str("\\neq "),
                '≈' => out.push_str("\\approx "),
                '∝' => out.push_str("\\propto "),
                '∈' => out.push_str("\\in "),
                '∉' => out.push_str("\\notin "),
                '⊂' => out.push_str("\\subset "),
                '∪' => out.push_str("\\cup "),
                '∩' => out.push_str("\\cap "),
                '−' => out.push('-'),
                _ => out.push(c),
            }
            out
        })
}

pub(super) fn write_formula_index(
    output_dir: &Path,
    source_pdf: &Path,
    page_count: usize,
    candidates: &[FormulaReportRecord],
) -> anyhow::Result<()> {
    let debug_dir = output_dir.join("debug").join("formulas");
    std::fs::create_dir_all(&debug_dir)
        .with_context(|| format!("Failed to create formula debug dir {}", debug_dir.display()))?;

    let pages_with_candidates = candidates
        .iter()
        .map(|candidate| candidate.page)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let local_candidate_count = candidates
        .iter()
        .filter(|candidate| matches!(candidate.status, FormulaStatus::LocalCandidate))
        .count();
    let needs_review_count = candidates
        .iter()
        .filter(|candidate| matches!(candidate.status, FormulaStatus::NeedsReview))
        .count();
    let backend_recovered_count = candidates
        .iter()
        .filter(|candidate| matches!(candidate.status, FormulaStatus::BackendRecovered))
        .count();
    let emitted_count = candidates
        .iter()
        .filter(|candidate| candidate.emitted)
        .count();
    let review_block_count = candidates
        .iter()
        .filter(|candidate| candidate.review_block)
        .count();

    let index = FormulaDebugIndex {
        schema_version: 1,
        source_pdf: source_pdf.display().to_string(),
        page_count,
        candidate_count: candidates.len(),
        pages_with_candidates,
        local_candidate_count,
        needs_review_count,
        backend_recovered_count,
        emitted_count,
        review_block_count,
        candidates,
    };

    let path = debug_dir.join("index.json");
    let json = serde_json::to_string_pretty(&index)?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write formula index {}", path.display()))
}

/// Parameters for [`write_formula_debug`] gathered at the call site.
pub(super) struct FormulaDebugParams<'a> {
    pub pdf_path: &'a Path,
    pub crop_dir: &'a Path,
    pub crop_rel_dir: &'a str,
    pub page_num: usize,
    pub candidates: &'a mut [FormulaCandidate],
    pub dpi: u32,
    pub sidecar: Option<&'a dyn FormulaSidecar>,
    pub write_json: bool,
}

pub(super) fn write_formula_debug(params: FormulaDebugParams<'_>) -> anyhow::Result<()> {
    let FormulaDebugParams {
        pdf_path,
        crop_dir,
        crop_rel_dir,
        page_num,
        candidates,
        dpi,
        sidecar,
        write_json,
    } = params;
    std::fs::create_dir_all(crop_dir)
        .with_context(|| format!("Failed to create formula crop dir {}", crop_dir.display()))?;

    if !candidates.is_empty() {
        let document = mupdf::Document::open(pdf_path).with_context(|| {
            format!(
                "Failed to open {} for formula rendering",
                pdf_path.display()
            )
        })?;
        let page = document.load_page(page_num as i32).with_context(|| {
            format!("Failed to load page {} for formula rendering", page_num + 1)
        })?;

        for candidate in candidates.iter_mut() {
            let filename = format!(
                "page{}_formula{}.png",
                page_num + 1,
                candidate.formula_index + 1
            );
            let abs_path = crop_dir.join(&filename);
            if let Some(bytes) = crate::figure::render::render_bbox_png(&page, candidate.bbox, dpi)
                .with_context(|| format!("Failed to render formula crop {filename}"))?
            {
                std::fs::write(&abs_path, bytes).with_context(|| {
                    format!("Failed to write formula crop {}", abs_path.display())
                })?;
                candidate.crop_path = Some(format!("{crop_rel_dir}/{filename}"));
                if let Some(sidecar) = sidecar {
                    if should_send_to_formula_sidecar(candidate) {
                        let attempt = sidecar.recognize(&abs_path);
                        if matches!(attempt.status, FormulaSidecarStatus::Recovered) {
                            let latex = attempt.latex.as_deref().unwrap_or("");
                            if recovered_latex_is_sane(latex, candidate) {
                                candidate.latex = attempt.latex.clone();
                                candidate.status = FormulaStatus::BackendRecovered;
                                candidate.backend = attempt.backend.clone();
                                candidate.sidecar.sanity = Some("passed".into());
                            } else {
                                candidate.sidecar.sanity = Some("rejected:bad-output".into());
                            }
                        }
                        candidate.sidecar = attempt;
                    } else {
                        let reason = formula_sidecar_rejection_reason(candidate)
                            .unwrap_or("candidate rejected by sidecar policy");
                        candidate.sidecar = FormulaSidecarAttempt::rejected_by_policy(reason);
                    }
                }
            }
        }
    }

    if write_json && !candidates.is_empty() {
        let debug_dir = crop_dir
            .parent()
            .filter(|parent| parent.file_name().is_some_and(|name| name == "debug"))
            .map(|_| crop_dir.to_path_buf())
            .unwrap_or_else(|| {
                crop_dir
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("debug")
                    .join("formulas")
            });
        std::fs::create_dir_all(&debug_dir).with_context(|| {
            format!("Failed to create formula debug dir {}", debug_dir.display())
        })?;
        let path = debug_dir.join(format!("page{}.json", page_num + 1));
        let json = serde_json::to_string_pretty(candidates)?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write formula debug {}", path.display()))?;
    }

    Ok(())
}

pub(super) fn should_send_to_formula_sidecar(candidate: &FormulaCandidate) -> bool {
    formula_sidecar_rejection_reason(candidate).is_none()
}

pub(super) fn formula_sidecar_rejection_reason(
    candidate: &FormulaCandidate,
) -> Option<&'static str> {
    if candidate.confidence < 65 {
        return Some("candidate below sidecar confidence threshold");
    }

    if is_visual_only_formula_candidate(candidate) {
        if visual_candidate_is_ocr_friendly(candidate) {
            return None;
        }
        return Some("visual-only crop too wide or ambiguous for sidecar OCR");
    }

    let text = candidate.source_text.trim();
    if text.is_empty() {
        return None;
    }
    if text.contains("<EOS>") || text.contains("<pad>") {
        return Some("candidate contains model-special prose tokens");
    }
    if candidate.equation_number.is_some() {
        return None;
    }

    let word_count = text.split_whitespace().count();
    let relation_count = text
        .chars()
        .filter(|c| matches!(c, '=' | '<' | '>' | '≤' | '≥'))
        .count();
    if relation_count == 0 {
        return Some("candidate has no formula relation operator");
    }
    if word_count > 14 {
        return Some("candidate has too many words for sidecar OCR");
    }
    if looks_like_definition_line(text) {
        return Some("candidate looks like a variable definition line");
    }
    if looks_like_table_range_comparison(text, relation_count) {
        return Some("candidate looks like a table/range comparison");
    }
    if looks_like_standards_table_or_prose_line(text) {
        return Some("candidate looks like standards table/prose content");
    }

    let stopword_count = text
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "the"
                    | "and"
                    | "or"
                    | "of"
                    | "to"
                    | "in"
                    | "we"
                    | "with"
                    | "for"
                    | "by"
                    | "is"
                    | "are"
                    | "this"
                    | "that"
                    | "as"
                    | "on"
                    | "from"
                    | "at"
                    | "be"
                    | "all"
                    | "used"
                    | "using"
                    | "have"
                    | "has"
            )
        })
        .count();
    if word_count >= 10 && stopword_count >= 3 && relation_count <= 1 {
        return Some("candidate looks like prose with an inline relation");
    }

    let math_score = text
        .chars()
        .filter(|c| {
            matches!(
                c,
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
                    | '^'
                    | '_'
            )
        })
        .count();

    if relation_count >= 2 || math_score >= 3 || (candidate.confidence >= 85 && word_count <= 10) {
        None
    } else {
        Some("candidate below sidecar math-content threshold")
    }
}

pub(super) fn is_visual_only_formula_candidate(candidate: &FormulaCandidate) -> bool {
    candidate.source_text.trim().is_empty()
        || candidate.backend.as_deref() == Some("visual-page-render")
        || candidate.reason.contains("visual-isolated-equation-band")
}

pub(super) fn visual_candidate_is_ocr_friendly(candidate: &FormulaCandidate) -> bool {
    let width = candidate.bbox.x1 - candidate.bbox.x0;
    let height = candidate.bbox.y1 - candidate.bbox.y0;
    if width <= 0.0 || height <= 0.0 {
        return false;
    }

    let aspect_ratio = width / height;
    if aspect_ratio > 24.0 {
        return false;
    }

    let broad_horizontal_band = width > 420.0
        && (candidate.source_text.trim().is_empty()
            || candidate.reason.contains("horizontal-rule"));
    !broad_horizontal_band
}

pub(super) fn looks_like_definition_line(text: &str) -> bool {
    let Some((_, rhs)) = text.split_once('=') else {
        return false;
    };
    let rhs_word_count = rhs.split_whitespace().count();
    if rhs_word_count < 4 {
        return false;
    }
    let rhs_lower = rhs.to_ascii_lowercase();
    let definition_terms = [
        "angle",
        "factor",
        "including",
        "margin",
        "object",
        "plates",
        "sling",
        "table",
        "thickness",
        "weight",
    ];
    definition_terms.iter().any(|term| rhs_lower.contains(term))
}

pub(super) fn looks_like_table_range_comparison(text: &str, relation_count: usize) -> bool {
    !text.contains('=') && relation_count >= 2 && text.split_whitespace().count() >= 4
}

pub(super) fn recovered_latex_is_sane(latex: &str, candidate: &FormulaCandidate) -> bool {
    if latex.is_empty() {
        return false;
    }

    let height = (candidate.bbox.y1 - candidate.bbox.y0).max(1.0);
    let width = (candidate.bbox.x1 - candidate.bbox.x0).max(1.0);
    let _expected_chars = (height * width / 100.0) as usize;

    // 1. Excessive backslash density: more than ~8 per height-point
    let backslash_count = latex.matches('\\').count();
    if backslash_count > (height as usize) * 8 && latex.len() > 50 {
        return false;
    }

    // 2. Repeated delimiter noise patterns
    let repeated_left = latex.matches("\\left|").count() + latex.matches("\\left\\|").count();
    let _repeated_array = latex.matches("\\begin{array}").count();
    if repeated_left > 3 && latex.len() > 100 {
        return false;
    }

    // 3. Overlong LaTeX for a small crop (more than 50 chars per point of height)
    if latex.len() > (height as usize) * 50 && width < 500.0 {
        return false;
    }

    // 4. Text-heavy recovered LaTeX with long English words not in source
    if !candidate.source_text.trim().is_empty() {
        let source_lower = candidate.source_text.to_ascii_lowercase();
        let latex_lower = latex.to_ascii_lowercase();
        let long_words_in_latex: Vec<&str> = latex_lower
            .split_whitespace()
            .filter(|w| w.len() > 8 && w.chars().all(|c| c.is_ascii_alphabetic()))
            .collect();
        let unexpected_long_words = long_words_in_latex
            .iter()
            .filter(|w| !source_lower.contains(*w))
            .count();
        if unexpected_long_words >= 3 {
            return false;
        }
    }

    // 5. Excessive `\\stackrel`/`\\overset` stacking
    let stack_count = latex.matches("\\stackrel").count()
        + latex.matches("\\overset").count()
        + latex.matches("\\widetilde").count()
        + latex.matches("\\widehat").count();
    if stack_count > 5 && source_text_length(candidate) < 30 {
        return false;
    }

    true
}

pub(super) fn source_text_length(candidate: &FormulaCandidate) -> usize {
    candidate.source_text.trim().len()
}

pub(super) fn looks_like_standards_table_or_prose_line(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let phrase_terms = [
        "defined as",
        "environmental criteria",
        "heading in degrees",
        "linear wave theory",
        "minimum required",
        "minimum tipping angle",
        "not applicable",
        "operational criteria",
        "seafastening force",
        "significant wave",
        "solitary wave theory",
        "upper bound",
        "wave period",
    ];
    if phrase_terms.iter().any(|term| lower.contains(term)) {
        return true;
    }

    let high_risk_single_terms = [
        "acceleration",
        "equation",
        "month",
        "n/a",
        "return",
        "see [",
        "tonnes",
        "year",
    ];
    if high_risk_single_terms
        .iter()
        .any(|term| lower.contains(term))
    {
        return true;
    }

    let table_terms = [
        "any",
        "barge",
        "bridles",
        "cargo",
        "category",
        "criteria",
        "days",
        "derate",
        "during",
        "equipment",
        "height",
        "hence",
        "lashing",
        "links",
        "load",
        "objects",
        "pennants",
        "plates",
        "shackles",
        "sockets",
        "smys",
        "towlines",
        "upend",
        "vessels",
        "visual",
    ];
    let matches = table_terms
        .iter()
        .filter(|term| lower.contains(*term))
        .count();
    matches >= 2
}

pub(super) fn warn_on_formula_candidate_summary(
    options: &ConvertOptions,
    page_count: usize,
    candidate_count: usize,
) {
    if candidate_count == 0 || options.hybrid.is_on() {
        return;
    }
    if matches!(
        options.effective_formula_mode(),
        FormulaMode::Auto | FormulaMode::Hybrid
    ) {
        eprintln!(
            "  warning: detected {candidate_count} formula candidate(s) across {page_count} page(s); use `--debug-formulas` to inspect crops or `--hybrid docling --formulas hybrid` for formula enrichment.",
        );
    }
}
