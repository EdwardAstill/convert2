//! Hybrid backend: delegate PDF pages that the local pipeline cannot handle
//! well (formulas, complex tables, scans) to an external service.
//!
//! Architecture (Phase 2b):
//!
//! 1. The local mupdf pipeline runs first and produces a `Document` with
//!    per-page `Block`s, images already saved to disk.
//! 2. [`apply_to_document`] iterates the pages, asks [`triage::should_route`]
//!    (or the policy override) whether each page should go to the backend,
//!    extracts the qualifying pages as single-page PDFs via
//!    [`page_extract::extract_page_as_pdf_bytes`], uploads each via
//!    [`client::DoclingClient::convert_bytes_to_markdown`], and stashes the
//!    returned markdown on `page.override_markdown`.
//! 3. The renderer honours `override_markdown`: when set, it emits that
//!    markdown verbatim for the page instead of serialising the local
//!    `blocks`. Images saved by the local pipeline remain on disk; nothing
//!    references them from routed pages, which is the current Phase 2b
//!    trade-off (documented in the plan).
//!
//! Backend failures are logged and skipped page-by-page: a single timeout
//! never kills the whole document.

pub mod client;
pub mod page_extract;
pub mod triage;

use std::path::Path;
use std::time::Duration;

use crate::document::types::Document;
use crate::error::VtvResult;

/// Routing policy for `--hybrid docling`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingPolicy {
    /// Per-page triage decides (default).
    Auto,
    /// Route every page, regardless of triage. Useful for tests and for
    /// users who want uniform Docling-quality output across a document.
    All,
}

/// Stats returned after a hybrid run, surfaced to `--verbose`.
#[derive(Debug, Default)]
pub struct HybridStats {
    pub pages_total: usize,
    pub pages_routed: usize,
    pub pages_failed: usize,
}

/// Augment `doc` in place by routing triage-qualifying pages through the
/// external Docling backend.
///
/// Per-page failures (network error, HTTP error, empty response) are logged
/// to stderr but do not abort — the page simply keeps its local rendering.
pub fn apply_to_document(
    doc: &mut Document,
    source_pdf: &Path,
    policy: RoutingPolicy,
    base_url: &str,
    timeout: Duration,
    verbose: bool,
) -> VtvResult<HybridStats> {
    let mut stats = HybridStats {
        pages_total: doc.pages.len(),
        ..Default::default()
    };
    let client = client::DoclingClient::new(base_url, timeout);

    for page in doc.pages.iter_mut() {
        let should = matches!(policy, RoutingPolicy::All) || triage::should_route(page);
        if !should {
            continue;
        }

        let bytes = match page_extract::extract_page_as_pdf_bytes(source_pdf, page.page_num) {
            Ok(b) => b,
            Err(e) => {
                stats.pages_failed += 1;
                eprintln!(
                    "  hybrid: page {}: extract failed, keeping local output ({e})",
                    page.page_num + 1
                );
                continue;
            }
        };

        let filename = format!("page{}.pdf", page.page_num + 1);
        match client.convert_bytes_to_markdown(bytes, &filename) {
            Ok(md) => {
                if verbose {
                    eprintln!(
                        "  hybrid: page {} routed to {} (got {} bytes of md)",
                        page.page_num + 1,
                        base_url,
                        md.len()
                    );
                }
                page.override_markdown = Some(md);
                stats.pages_routed += 1;
            }
            Err(e) => {
                stats.pages_failed += 1;
                eprintln!(
                    "  hybrid: page {}: backend call failed, keeping local output ({e})",
                    page.page_num + 1
                );
            }
        }
    }

    Ok(stats)
}
