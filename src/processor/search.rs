use std::path::Path;

use anyhow::Context;
use mupdf::{Document, Quad, TextPageFlags};
use serde::Serialize;

use crate::cli::SearchArgs;
use crate::ocr::{self, OcrDecision};

#[derive(Debug, Serialize)]
struct SearchReport {
    source: String,
    needle: String,
    page_count: usize,
    ocr: OcrDecision,
    matches: Vec<SearchPageMatch>,
}

#[derive(Debug, Serialize)]
struct SearchPageMatch {
    page: usize,
    hit_count: usize,
    quads: Vec<QuadReport>,
}

#[derive(Debug, Serialize)]
struct QuadReport {
    ul: PointReport,
    ur: PointReport,
    ll: PointReport,
    lr: PointReport,
}

#[derive(Debug, Serialize)]
struct PointReport {
    x: f32,
    y: f32,
}

pub fn run(args: &SearchArgs) -> anyhow::Result<()> {
    let prepared = ocr::prepare_pdf(&args.input, &args.ocr.clone().into_config(), args.verbose)?;
    let report = search_pdf(
        &prepared.effective_path,
        &args.input,
        &args.needle,
        prepared.decision,
    )?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report);
    }
    Ok(())
}

fn search_pdf(
    path: &Path,
    source_path: &Path,
    needle: &str,
    ocr: OcrDecision,
) -> anyhow::Result<SearchReport> {
    let path_str = path.to_string_lossy();
    let doc = Document::open(path_str.as_ref())
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let page_count = doc
        .page_count()
        .with_context(|| format!("Failed to count pages in {}", path.display()))?
        as usize;

    let mut matches = Vec::new();
    for page_num in 0..page_count {
        let page = doc
            .load_page(page_num as i32)
            .with_context(|| format!("Failed to load page {}", page_num + 1))?;
        let text_page = page
            .to_text_page(TextPageFlags::empty())
            .with_context(|| format!("Failed to build text page {}", page_num + 1))?;
        let quads = text_page
            .search(needle)
            .with_context(|| format!("Failed to search page {}", page_num + 1))?;
        if quads.is_empty() {
            continue;
        }
        matches.push(SearchPageMatch {
            page: page_num + 1,
            hit_count: quads.len(),
            quads: quads.iter().map(quad_report).collect(),
        });
    }

    Ok(SearchReport {
        source: source_path.display().to_string(),
        needle: needle.to_string(),
        page_count,
        ocr,
        matches,
    })
}

fn quad_report(quad: &Quad) -> QuadReport {
    QuadReport {
        ul: point_report(quad.ul.x, quad.ul.y),
        ur: point_report(quad.ur.x, quad.ur.y),
        ll: point_report(quad.ll.x, quad.ll.y),
        lr: point_report(quad.lr.x, quad.lr.y),
    }
}

fn point_report(x: f32, y: f32) -> PointReport {
    PointReport { x, y }
}

fn print_human_report(report: &SearchReport) {
    println!("source: {}", report.source);
    println!("needle: {}", report.needle);
    println!("pages: {}", report.page_count);
    let total_hits: usize = report.matches.iter().map(|m| m.hit_count).sum();
    println!("hits: {total_hits}");
    for page_match in &report.matches {
        println!("page {}: {} hit(s)", page_match.page, page_match.hit_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupdf::Point;

    #[test]
    fn quad_report_preserves_points() {
        let quad = Quad::new(
            Point { x: 1.0, y: 2.0 },
            Point { x: 3.0, y: 4.0 },
            Point { x: 5.0, y: 6.0 },
            Point { x: 7.0, y: 8.0 },
        );
        let report = quad_report(&quad);
        assert_eq!(report.ul.x, 1.0);
        assert_eq!(report.lr.y, 8.0);
    }
}
