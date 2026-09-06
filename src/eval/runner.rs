use crate::batch;
use crate::config::{ConvertOptions, FigureMode};
use crate::eval::fixtures::FixtureFile;
use crate::eval::metrics::{
    aggregate, apply_formula_debug_metrics, compute_page_metrics, DocMetrics,
    FormulaDebugPageMetrics, PageMetrics,
};
use crate::pipeline::process_pdf_to_document;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DocResult {
    pub doc_name: String,
    pub metrics: DocMetrics,
    pub error: Option<String>,
}

pub fn run_eval(fixtures: &[FixtureFile]) -> Vec<DocResult> {
    fixtures.iter().map(run_one).collect()
}

fn run_one(fixture: &FixtureFile) -> DocResult {
    let pdf_path = fixture.pdf_path();
    let doc_name = pdf_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let output_base = eval_output_base(&doc_name);

    let options = eval_convert_options(output_base.clone());

    let document = match process_pdf_to_document(&pdf_path, &options) {
        Ok(document) => document,
        Err(err) => {
            return DocResult {
                doc_name,
                metrics: DocMetrics::default(),
                error: Some(format!("{:#}", anyhow::Error::from(err))),
            };
        }
    };

    let formula_debug = load_formula_debug_pages(&pdf_path, &output_base).unwrap_or_default();
    let mut page_metrics: Vec<PageMetrics> = Vec::new();
    for expectation in &fixture.pages {
        let Some(page) = document
            .pages
            .iter()
            .find(|page| page.page_num + 1 == expectation.page)
        else {
            continue;
        };
        let mut metrics = compute_page_metrics(page, expectation);
        if let Some(debug_page) = formula_debug.get(&expectation.page) {
            apply_formula_debug_metrics(&mut metrics, expectation, debug_page);
        }
        page_metrics.push(metrics);
    }

    DocResult {
        doc_name,
        metrics: aggregate(&page_metrics),
        error: None,
    }
}

#[derive(Debug, Deserialize)]
struct FormulaDebugIndex {
    candidates: Vec<FormulaDebugRecord>,
}

#[derive(Debug, Deserialize)]
struct FormulaDebugRecord {
    page: usize,
    emitted: bool,
    latex: Option<String>,
    source_text: String,
}

fn load_formula_debug_pages(
    pdf_path: &Path,
    output_base: &Path,
) -> anyhow::Result<BTreeMap<usize, FormulaDebugPageMetrics>> {
    let output_dir = batch::conversion_output_dir_for(pdf_path, Some(output_base), false);
    let path = output_dir.join("debug").join("formulas").join("index.json");
    let json = std::fs::read_to_string(&path)?;
    let index: FormulaDebugIndex = serde_json::from_str(&json)?;

    let mut pages = BTreeMap::<usize, FormulaDebugPageMetrics>::new();
    for candidate in index.candidates {
        let page = pages.entry(candidate.page).or_default();
        page.candidates += 1;
        if candidate.emitted {
            page.emitted += 1;
        }
        if let Some(latex) = candidate.latex.filter(|latex| !latex.trim().is_empty()) {
            page.latex_values.push(latex);
        } else if !candidate.source_text.trim().is_empty() {
            page.latex_values.push(candidate.source_text);
        }
    }
    Ok(pages)
}

fn eval_output_base(doc_name: &str) -> PathBuf {
    let safe_name = doc_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    std::env::temp_dir()
        .join("pdfp-eval-media")
        .join(std::process::id().to_string())
        .join(safe_name)
}

fn eval_convert_options(output_dir: PathBuf) -> ConvertOptions {
    let _ = std::fs::remove_dir_all(&output_dir);

    ConvertOptions {
        output: Some(output_dir),
        images: true,
        no_images: false,
        figures: Some(FigureMode::Snapshot),
        figure_dpi: 96,
        debug_formulas: true,
        ..ConvertOptions::default()
    }
}
