# Testing

What is tested, how to run the tests, and which paths remain unverified.

## Automated tests (`cargo test`)

| Layer | Command | Count | Runtime |
| --- | --- | ---: | ---: |
| Unit tests (inline `#[cfg(test)]`) | `cargo test --bin cnv` | 140 | ~0.05 s |
| Golden lorem smoke | `cargo test --test golden` | 1 | ~0.05 s |
| Golden corpus sweep | `cargo test --test golden -- --ignored golden_corpus_sweep` | 1 (iterates 13 PDFs) | ~16 s |
| Golden snapshot diff (attention page 1) | `cargo test --test golden -- --ignored golden_snapshot_attention_page_1` | 1 | ~3 s |
| Hybrid — `httpmock` | `cargo test --test hybrid` | 3 | ~0.5 s |
| Hybrid — live docling-serve (see below) | `DOCLING_URL=… cargo test --test hybrid -- --ignored hybrid_live` | 1 | variable |

All default-flag tests pass on the current tree:

```
cargo test
# → unit (140), golden_lorem_quick (1), hybrid mock (3)  — all pass

cargo test --test golden -- --ignored
# → corpus sweep (1) + snapshot diff (1) — both pass

cargo clippy --all-targets -- -D warnings
# → clean

cargo check --features pdfium-metadata
# → compiles
```

### What the tests actually cover

- **XY-Cut++ reading order** — 16 unit tests in `src/layout/xycut.rs::tests` exercising two-column pages, spanning titles, narrow-outlier retry, cross-layout pre-masking, and degenerate inputs.
- **Classifier heuristics** — tests in `src/layout/classifier.rs::tests` covering each `BlockKind` detection rule; the Phase 3 metadata tests verify struct-tree overrides and bold-at-body-size promotion using a mock `PageMetadata`.
- **Metadata lookup** — 6 tests in `src/pdf/metadata.rs::tests` covering overlap scoring, bbox matching, and the stub loader.
- **PDF extraction subscript/superscript logic** — 20+ tests in `src/pdf/extractor.rs::tests` exercising classify_char_script, group_into_text_rows, and real-world traces from AISC-360.
- **Markdown renderer** — tests in `src/render/markdown.rs::tests` covering heading/paragraph/list/table emission, section splitting, scanned-page warnings, and the Phase 2b `override_markdown` path (implicitly via hybrid integration tests).
- **LaTeX→Typst translator** — 20+ unit tests in `src/typst/latex2typst.rs::tests` across Greek, big operators, matrices, frac, scripts, aligned environments, etc.
- **Hybrid triage** — 8 unit tests in `src/hybrid/triage.rs::tests` covering math-density threshold, table detection, low-density detection, and the fact that running-footer text is excluded from the math count.
- **Hybrid client parsing** — 5 unit tests on `ConvertResponse` deserialisation, all documented fallback keys (`md_content`, `content_md`, nested under `document`).
- **Hybrid end-to-end** (via `httpmock` in `tests/hybrid.rs`) — three scenarios:
  - Mock server returns canned markdown → output contains it; mock hit count asserted.
  - Mock server returns 502 → per-page failure is logged and the local path's output is kept; cnv exits 0.
  - `--hybrid off` produces byte-identical output to the Phase 1 snapshot — regression guard across all later phases.
- **Corpus sweep** — invokes the built binary against 13 real PDFs (arXiv ML papers + OpenDataLoader fixtures including a Chinese scan and an Italian invoice). Asserts exit 0, non-empty markdown, and ≥ 1 image extracted for figure-heavy papers.
- **Snapshot** — `tests/snapshots/attention_page_1.md` is the authoritative reference for the local path's reading order + classification on a two-column academic paper. Regenerate with `GOLDEN_UPDATE=1`.

## Test PDFs

Under `papers/`:

| File | Profile |
| --- | --- |
| `attention.pdf` | 2-col ML; inline math; figures |
| `bert.pdf` | 2-col ML; many small figures |
| `clip.pdf` | 2-col ML; figure-dense |
| `gpt3.pdf` | long paper (60+ pages) |
| `resnet.pdf` | 2-col ML; vector-drawn diagrams (intentionally skipped by the image assertion — see the test source for why) |
| `math-number-theory.pdf` | display equations |
| `physics-hep.pdf` | dense physics math |
| `survey-llm.pdf` | figure-heavy survey |

Under `papers/golden/` (fixtures copied from OpenDataLoader's samples):

| File | Profile |
| --- | --- |
| `lorem.pdf` | trivial prose — fast smoke |
| `1901.03003.pdf` | arXiv layout reference |
| `2408.02509v1.pdf` | arXiv layout reference |
| `chinese_scan.pdf` | scanned Chinese document (local path produces near-empty; hybrid OCR path is the fix) |
| `issue-336-conto-economico-bialetti.pdf` | real-world Italian invoice |
| `pdfua-1-reference-suite-1-1/*.pdf` | 10 tagged PDF/UA-1 samples (for Phase 3 tagged-PDF verification) |

## Unverified paths

Two deliverables landed with automated tests that use mocks or stubs but have **not been exercised against the real external systems they target**. The code compiles, clippy is clean, and unit tests pass, but the live behaviour is an open question until someone runs the commands below.

### 1. `--hybrid docling` against a real docling-serve

**What's verified:** request wiring (`reqwest::blocking` + multipart), response parsing for several plausible schemas (`md_content` top-level, `md_content` nested under `document`, `content_md` fallback), per-page failure tolerance, regression guard that `--hybrid off` is unchanged.

**What's not verified:** that the guessed request shape (`POST /v1/convert/file` with multipart field `files` and extra form fields `to_formats=md`, `do_ocr=true`, `do_table_structure=true`, `do_formula_enrichment=true`) matches what docling-serve actually accepts; and that the response JSON shape matches what we parse. If docling-serve's schema has moved, the parser in `src/hybrid/client.rs::ConvertResponse` may need to add a field.

**How to close the gap**:

With Docker:

```bash
# 1. Start docling-serve (pulls models on first run; ~5 min cold start).
docker run -d --name docling -p 5001:5001 \
  quay.io/docling-project/docling-serve:latest

# 2. Verify it's up.
curl -s http://localhost:5001/v1/healthz

# 3. Run the live integration test.
DOCLING_URL=http://localhost:5001 \
  cargo test --test hybrid -- --ignored hybrid_live

# 4. If hybrid_live fails, run cnv manually and inspect the raw response:
RUST_LOG=debug cargo run -- papers/math-number-theory.pdf \
  --hybrid docling --hybrid-url http://localhost:5001 \
  --hybrid-policy all --verbose -o /tmp/cnv-live-math
```

With `uv`/`pip` (requires Python 3.10–3.12 — not 3.14):

```bash
uv venv --python 3.11 /tmp/docling-venv
source /tmp/docling-venv/bin/activate
uv pip install "docling-serve[ui]"
docling-serve run --port 5001
# ...then run the `cargo test --test hybrid -- --ignored hybrid_live` above.
```

Symptoms of a schema mismatch and where to fix them:

- Test fails with "response contained no markdown content" → docling returned JSON that does not match any of the three keys we accept. Add a new branch to `ConvertResponse::extract_markdown` in `src/hybrid/client.rs`.
- Test fails with "response was not valid JSON" → content-type or body isn't what we expected. Inspect via `curl` and widen the parser.
- Test fails with "HTTP 400"-class → the request body is wrong. Inspect docling-serve's OpenAPI at `http://localhost:5001/docs` and correct the multipart field names in `DOCLING_OPTIONS`.

### 2. `--features pdfium-metadata` at runtime

**What's verified:** the feature flag gates the `pdfium-render` dependency out of the default build; the feature-on build compiles clean against `pdfium-render 0.9.0` (verified by reading that crate's source to match `PdfFont::family()`, `PdfFont::weight() -> Result<PdfFontWeight, _>`, `PdfQuadPoints::{left,right,top,bottom}`); the classifier correctly uses `PageMetadata` via a mock in unit tests.

**What's not verified:** that `Pdfium::bind_to_system_library()` actually finds a real `libpdfium.so` on an end-user machine; that the guessed y-axis flip (pdfium bottom-left → our top-left) produces bboxes that actually align with mupdf's bboxes for the same page regions; that `PdfFontWeight::Custom(v)` values in the wild stay inside 100..=900.

**How to close the gap**:

```bash
# Arch
sudo pacman -S pdfium-binaries

# Debian / Ubuntu
curl -L https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz \
  | sudo tar -xz -C /usr/local

# macOS
brew install pdfium

# Verify the binding works:
cargo run --features pdfium-metadata -- \
  papers/golden/pdfua-1-reference-suite-1-1/some-tagged.pdf \
  -o /tmp/cnv-pdfium-test --verbose

# Eyeball the output markdown for heading hierarchy.
# If pdfium failed to bind, you will see:
#   pdfium-metadata: page N metadata unavailable (bind_to_system_library: ...)
# ...and the classifier silently degrades to size-only (this is the documented
# graceful-degrade path, so it is a safe failure mode — not a crash).
```

If the feature builds but the output does not improve over the default build on tagged PDFs, the most likely culprit is that the bbox from pdfium disagrees with mupdf's bbox and the classifier's `font_for_bbox` lookup never finds a match. Inspect by adding a `dbg!(&metadata.fonts)` call in `src/main.rs::process_pdf` and checking whether the recorded rects overlap the text blocks.

### 3. Manual eyeball on real PDFs (always do this)

Automated tests can only assert on non-empty, non-panicking output. Quality is a human judgment. Before shipping a release:

```bash
rm -rf /tmp/cnv-eyeball && mkdir /tmp/cnv-eyeball
cargo run --release -- papers/ -o /tmp/cnv-eyeball --verbose
ls -la /tmp/cnv-eyeball/*/
# then read a few of the generated .md files in your editor.
```

Things to look for:

- Two-column papers: title → authors → abstract → body in correct order, not interleaved by column.
- Math-heavy pages: if routed to Docling, display equations should appear as `$$ ... $$`. If not routed, Unicode math characters should still be present (or the page should have silently dropped any glyph without a ToUnicode map, which is the documented local-path limit — see `docs/pdf-internals.md` § "Fonts, encodings, and why text sometimes vanishes").
- Figures: each page with a raster figure in the source PDF should produce `![image](images/pageN_imgM.png)` with a real PNG file on disk.
- Tables: GFM pipe tables. Missing cells are OK (the classifier's grid detector is best-effort); garbage text in cells is not OK.
- Page markers: `<!-- page:N -->` separators present, hidden on rendered display.

## How to add a new test

- **Unit test** — put it in a `#[cfg(test)] mod tests` block in the module under test. Reach for this first; unit tests are the fastest, most precise diagnostic.
- **Integration test** — new file under `tests/`. Use `env!("CARGO_BIN_EXE_cnv")` to locate the binary (cargo rebuilds it automatically before running integration tests). Mark slow tests `#[ignore]` with a doc-comment explaining how to invoke them.
- **New test PDF** — drop under `papers/` if it's an ML paper / typical document, or `papers/golden/` if it comes from an upstream fixture set. Append to `CORPUS_PATHS` in `tests/golden.rs`. If the PDF should have images, append to `EXPECTS_IMAGES` too. If it's a tagged PDF intended for Phase 3 verification, it can stay in `papers/golden/pdfua-1-reference-suite-1-1/`.

## Known non-determinism

- mupdf's dominant-font-size detection bucketises to 0.5 pt, so two adjacent PDFs with 9.5 pt and 10 pt body text may classify differently depending on which bucket wins. Tests are stable because each PDF is processed in isolation.
- Temp file paths in `src/hybrid/page_extract.rs` use `std::process::id()` to avoid concurrent-test collisions, so running `cargo test -- --test-threads=1` is not required but is safer when iterating locally.
- Network errors in the live hybrid test are environment-dependent. The `hybrid_live` test is `#[ignore]` by design and is not part of CI-green criteria.
