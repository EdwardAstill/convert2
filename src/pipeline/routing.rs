//! Routing stage of the conversion pipeline: OCR warnings and hybrid
//! backend dispatch decisions.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;

use crate::config::{self, ConvertOptions};
use crate::document::types::{Document, Page};
use crate::hybrid;

pub(super) fn warn_on_scan_like_pages(
    pdf_path: &Path,
    options: &ConvertOptions,
    pages: &[Page],
    scan_report: &hybrid::triage::ScanReport,
) {
    let empty_page_count = pages.iter().filter(|p| p.blocks.is_empty()).count();
    if empty_page_count > 0 {
        eprintln!(
            "  warning: {} of {} pages have no extractable text (possibly scanned)",
            empty_page_count,
            pages.len()
        );
    }

    if !options.hybrid.is_on() && scan_report.likely_scan_like() {
        let next_step = if matches!(options.ocr.ocr, config::OcrMode::Off) {
            "Try `--ocr auto` for local OCR, or `--hybrid docling` for external assist."
        } else {
            "Check `--lang`, or try `--hybrid docling` for external assist."
        };
        eprintln!(
            "  warning: {} looks scan-heavy ({} image-only / {} low-density page(s), {} page(s) with readable text); local output may be poor. {}",
            pdf_path.display(),
            scan_report.image_only_pages,
            scan_report.low_density_pages,
            scan_report.pages_with_readable_text,
            next_step
        );
    }
}

pub(super) fn apply_hybrid_if_enabled(
    doc: &mut Document,
    pdf_path: &Path,
    options: &ConvertOptions,
    scan_report: &hybrid::triage::ScanReport,
) -> anyhow::Result<()> {
    if !options.hybrid.is_on() {
        return Ok(());
    }

    let policy = match options.hybrid_policy {
        config::HybridPolicy::Auto if scan_report.likely_scan_like() => {
            if options.verbose {
                eprintln!(
                    "  hybrid: document looks scan-heavy; upgrading auto policy to route all pages"
                );
            }
            hybrid::RoutingPolicy::All
        }
        config::HybridPolicy::Auto => hybrid::RoutingPolicy::Auto,
        config::HybridPolicy::All => hybrid::RoutingPolicy::All,
    };

    let stats = hybrid::apply_to_document(
        doc,
        pdf_path,
        policy,
        &options.hybrid_url,
        Duration::from_secs(options.hybrid_timeout_secs),
        options.hybrid_cache_dir.as_deref(),
        options.verbose,
    )
    .with_context(|| {
        format!(
            "hybrid backend ({}) failed for {}",
            options.hybrid_url,
            pdf_path.display()
        )
    })?;

    if options.verbose {
        eprintln!(
            "  hybrid: routed {}/{} pages ({} failed)",
            stats.pages_routed, stats.pages_total, stats.pages_failed
        );
        if stats.pages_cached > 0 {
            eprintln!("  hybrid: cache hits {}", stats.pages_cached);
        }
    }

    Ok(())
}
