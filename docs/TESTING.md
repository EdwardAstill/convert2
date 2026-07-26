# Testing

What is tested, how to run the tests, and which paths remain unverified.

Active scope note: the main `pdfp` binary is now a local PDF processor. Markdown conversion is the mature workflow; inspect, search, page operations, imposition, and resize are covered by focused processor tests.

## Automated tests (`cargo test`)

| Layer | Command | Count | Runtime |
| --- | --- | ---: | ---: |
| Unit tests (inline `#[cfg(test)]`) | `cargo test --bin pdfp` | broad inline suite | ~0.05 s |
| CLI help smoke tests | `cargo test --test cli_help` | one pass over every command path | ~0.05 s |
| Metadata CLI integration | `cargo test --test metadata` | show/set/clear, all writable fields, Unicode/empty strings, dates, XMP/signature handling, encrypted-PDF refusal | ~0.1 s |
| Processor command units | `cargo test processor::` | focused parser/order tests | ~0.05 s |
| Golden smoke/regression fixtures | `cargo test --test golden` | 4 (skip when fixtures are absent) | ~0.05 s+ |
| Golden corpus sweep | `cargo test --test golden -- --ignored golden_corpus_sweep` | 1 (iterates 13 PDFs) | ~16 s |
| Golden snapshot diff (attention page 1) | `cargo test --test golden -- --ignored golden_snapshot_attention_page_1` | 1 | ~3 s |
| PDF quality report | `bash scripts/quality-report.sh` | corpus summary + JSON report | corpus-dependent |
| Local OCR sidecar | `cargo test --test ocr` | OCR decisions, standalone OCR command, missing-tool behavior, fake-cache hit | ~0.5 s+ |
| Native formula OCR feature | `cargo test --features onnx-ocr --test formula_onnx` | ONNX feature gate, preprocessing tensor shape, vocab decode, sidecar parser/error paths | first run downloads ORT crates |
| Figure snapshots | `cargo test --test figure_snapshots` | `--figures snapshot`, `--figures none`, `--no-images` precedence, PNG output | ~1 s |
| Hybrid — `httpmock` | `cargo test --test hybrid` | 4 (skip when fixtures are absent) | ~0.5 s+ |
| Hybrid — live docling-serve (see below) | `DOCLING_URL=… cargo test --test hybrid -- --ignored hybrid_live` | 1 | variable |

The repository keeps `tests/` small and text-only. PDF fixtures, extracted images, and other large corpus assets live under ignored `test-corpus/`. Populate it with `scripts/fetch-papers.sh` plus any manually copied OpenDataLoader goldens before running the full corpus checks.

Default-flag tests pass on the current tree; fixture-backed integration tests print `SKIP` when `test-corpus/` is absent:

```
cargo test
# → unit tests pass; fixture-backed tests run or skip depending on test-corpus/

cargo test --test golden -- --ignored
# → corpus/snapshot tests run when test-corpus/ is populated

bash scripts/quality-report.sh
# → writes /tmp/pdfp-quality/report.json; exits 0 with SKIP if test-corpus/ is absent

PDFP_QUALITY_CORPUS=example/pdf PDFP_QUALITY_RECURSIVE=0 PDFP_QUALITY_OUT=target/quality-top \
  bash scripts/quality-report.sh
# → writes a top-level-only report over the 22 checked-in example PDFs

PDFP_QUALITY_CORPUS=example/pdf PDFP_QUALITY_RECURSIVE=1 PDFP_QUALITY_OUT=target/quality-recursive \
  bash scripts/quality-report.sh
# → writes a recursive report over all 44 checked-in example PDFs

bash scripts/example-audit.sh
# → writes target/example-audit/summary.md with quality, table, formula, figure, and scan signals

bash scripts/sidecar-audit.sh
# → writes target/sidecar-audit/summary.md; deterministic peers and optional sidecars skip cleanly when unavailable

scripts/generate-eval-fixtures.sh stage9-hard-images
# → writes ignored test-corpus/eval/stage9-hard-images.pdf from tracked Typst/SVG sources

cargo run --quiet -- eval tests/eval_fixtures/
# → reports heading/formula/table floors plus hard image/vector fixture metrics

cargo clippy --all-targets -- -D warnings
# → clean

cargo check --features pdfium-metadata
# → compiles

cargo test --test ocr
# → verifies OCR command construction, clean-PDF skip behavior, actionable missing-tool errors, inspect/search OCR provenance, and fake OCR cache hits
```

## Processor Command Smoke Tests

Run these against the checked-in `example/pdf` fixtures after changing the command dispatcher, page operations, imposition, or resize code:

```bash
cargo build

target/debug/pdfp convert example/pdf/golden__lorem.pdf -o target/compat-convert

target/debug/pdfp inspect example/pdf/golden__lorem.pdf --json \
  | jq '.page_count == 1'

target/debug/pdfp metadata show example/pdf/golden__lorem.pdf --json \
  | jq '.info | type == "object"'

target/debug/pdfp metadata set example/pdf/golden__lorem.pdf \
  -o target/lorem-metadata.pdf --title "Metadata Smoke" --no-touch-mod-date

target/debug/pdfp metadata clear target/lorem-metadata.pdf \
  -o target/lorem-metadata-cleared.pdf --fields title

target/debug/pdfp search example/pdf/attention.pdf Attention --json \
  | jq '(.matches | length) > 0'

target/debug/pdfp inspect example/pdf/golden__lorem.pdf --ocr auto --json \
  | jq '.ocr.status == "skipped" and .ocr.mode == "auto"'

target/debug/pdfp search example/pdf/attention.pdf Attention --ocr auto --json \
  | jq '(.matches | length) > 0'

target/debug/pdfp ocr example/pdf/golden__lorem.pdf \
  -o target/lorem.searchable.pdf --command definitely-missing-pdfp-ocr-command --json \
  | jq '.status == "skipped" and .mode == "auto"'

target/debug/pdfp doctor --json \
  | jq '.ocr.available | type == "boolean"'

target/debug/pdfp pages extract \
  example/pdf/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-04_Presentation.pdf \
  --pages 1-2 -o target/presentation-p1-p2.pdf

target/debug/pdfp pages reorder \
  example/pdf/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-04_Presentation.pdf \
  --pages 2,1 -o target/reordered.pdf

target/debug/pdfp pages merge \
  example/pdf/golden__lorem.pdf example/pdf/golden__lorem.pdf \
  -o target/merged.pdf

target/debug/pdfp impose 2up \
  example/pdf/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-04_Presentation.pdf \
  -o target/2up.pdf

target/debug/pdfp impose booklet \
  example/pdf/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-04_Presentation.pdf \
  -o target/booklet.pdf

target/debug/pdfp page resize example/pdf/golden__lorem.pdf \
  --paper a4 --fit contain -o target/lorem-a4.pdf

target/debug/pdfp page text example/pdf/golden__lorem.pdf \
  -o target/lorem-labelled.pdf --text "Text overlay smoke" \
  --pages 1 --x 36 --y 36 --origin top-left \
  --font helvetica-bold --font-size 18 --colour '#cc0000'
```

Expected checks:

- `pdfp inspect target/presentation-p1-p2.pdf --json | jq '.page_count == 2'`
- `pdfp inspect target/reordered.pdf --json | jq '.page_count == 2'`
- `pdfp inspect target/merged.pdf --json | jq '.page_count == 2'`
- `pdfp inspect target/2up.pdf --json | jq '.page_count == 4'`
- `pdfp inspect target/booklet.pdf --json | jq '(.page_count % 2) == 0'`
- `pdfp inspect target/lorem-a4.pdf --json | jq '.pages[0].width >= 594 and .pages[0].height >= 841'`
- `pdfp search target/lorem-labelled.pdf 'Text overlay smoke' --ocr off --json | jq '.matches[0].page == 1'`

Current processor limitations:

- `pdfp search` searches embedded PDF text unless `--ocr auto` or `--ocr force` is requested. Scanned pages also need OCRmyPDF, Tesseract, and the requested Tesseract language packs available locally.
- `pages merge` and `pages reorder` preserve page contents but do not yet guarantee document-level metadata, outlines, forms, or annotations.
- `impose` and `page resize` validate page count and geometry first; visual fidelity should still be checked by rendering sample outputs before release.

### What the tests actually cover

- **XY-Cut++ reading order** — 16 unit tests in `src/layout/xycut.rs::tests` exercising two-column pages, spanning titles, narrow-outlier retry, cross-layout pre-masking, and degenerate inputs.
- **CLI help** — `tests/cli_help.rs` runs `pdfp --help` plus every nested command help path, including `pdfp pages extract --help`, `pdfp impose booklet --help`, and `pdfp page text --help`.
- **Processor CLI and PDF operations** — page range parsing, inspect/search smoke checks, safe extract/delete/split, graft-based reorder/merge, booklet ordering, 2-up page count, A4 resize geometry, and searchable text overlays with page/font/colour/position assertions.
- **Metadata CLI** — `tests/metadata.rs` creates small PDFs with `lopdf` and verifies metadata show/set/clear round trips for every supported field, same-path refusal, Unicode and empty text strings, dates, XMP warnings, signed-PDF policy, and safe refusal of encrypted PDFs.
- **Classifier heuristics** — tests in `src/layout/classifier.rs::tests` covering each `BlockKind` detection rule; the Phase 3 metadata tests verify struct-tree overrides and bold-at-body-size promotion using a mock `PageMetadata`.
- **Metadata lookup** — 6 tests in `src/pdf/metadata.rs::tests` covering overlap scoring, bbox matching, and the stub loader.
- **PDF extraction subscript/superscript logic** — 20+ tests in `src/pdf/extractor.rs::tests` exercising classify_char_script, group_into_text_rows, and real-world traces from AISC-360.
- **Markdown renderer** — tests in `src/render/markdown.rs::tests` covering heading/paragraph/list/table emission, section splitting, scanned-page warnings, and the Phase 2b `override_markdown` path (implicitly via hybrid integration tests).
- **Figure snapshots** — tests in `src/figure/` cover candidate grouping and tiny/decorative rejection; `tests/figure_snapshots.rs` runs the real CLI against `attention.pdf` and checks rendered `_fig` PNGs, `--figures none`, and `--no-images` precedence.
- **Formula candidates** — tests in `src/formula/` cover centered equation-number detection, prose rejection, bbox clamping, and visual-only review heuristics; `tests/formulas.rs` runs the real CLI against `math-number-theory.pdf` and checks `--debug-formulas` JSON, rendered formula crops, auto-mode display math promotion, and local warning behavior.
- **Native formula OCR** — `tests/formula_onnx.rs` runs only with `--features onnx-ocr`. It checks the public ONNX module, 192×672 grayscale preprocessing tensor, vocabulary loading/token decoding, model-directory validation, and `--formula-sidecar` parsing for bare command, `cmd:`, and `onnx:` forms.
- **Conservative conversion** — `src/cli.rs` unit tests verify that `--conservative` resolves to embedded figures, layout tables, and formula audit mode. `tests/formulas.rs` checks that conservative conversion does not render heuristic formula blocks even if `--formulas local` is present.
- **Local OCR sidecar** — tests in `src/ocr/` and `tests/ocr.rs` cover OCRmyPDF argument construction, the standalone `pdfp ocr` command, triage that avoids clean born-digital PDFs, actionable missing-command failures for scan-heavy PDFs, JSON provenance for inspect/search, and cache-hit behavior using a fake OCR command.
- **Hybrid triage/cache** — tests in `src/hybrid/triage.rs::tests` and `src/hybrid/mod.rs::tests` covering math-density threshold, table detection, low-density detection, running-footer exclusion, and cache hits that bypass backend/PDF extraction.
- **Hybrid client parsing** — 5 unit tests on `ConvertResponse` deserialisation, all documented fallback keys (`md_content`, `content_md`, nested under `document`).
- **Hybrid end-to-end** (via `httpmock` in `tests/hybrid.rs`) — three scenarios:
  - Mock server returns canned markdown → output contains it; mock hit count asserted.
  - Mock server returns 502 → per-page failure is logged and the local path's output is kept; pdfp exits 0.
  - `--hybrid off` produces byte-identical output to the Phase 1 snapshot — regression guard across all later phases.
- **Corpus sweep** — invokes the built binary against 13 real PDFs (arXiv ML papers + OpenDataLoader fixtures including a Chinese scan and an Italian invoice). Asserts exit 0, non-empty markdown, and ≥ 1 image extracted for figure-heavy papers.
- **Snapshot** — `tests/snapshots/attention_page_1.md` is the authoritative reference for the local path's reading order + classification on a two-column academic paper. Regenerate with `GOLDEN_UPDATE=1`.
- **Sidecar audit** — `scripts/sidecar-audit.sh` compares native output with deterministic peers (`pdftotext`, PyMuPDF4LLM, pdfplumber, pdfminer.six, Camelot, Tabula, OCRmyPDF) plus optional ML sidecars. Missing commands/modules skip cleanly and the script still writes `target/sidecar-audit/summary.md`. MarkItDown fixture PDFs can be kept under ignored `test-corpus/third-party/markitdown/` for manual form-table comparisons; they are not required by CI.

## Test PDFs

Under ignored `test-corpus/`:

The quality harness also reads this ignored directory. Keep PDFs and extracted images out of git; `tests/quality.rs` verifies the harness reports a clear `SKIP` instead of pretending quality coverage exists when the corpus is absent. Override paths when needed:

```bash
PDFP_QUALITY_CORPUS=/path/to/test-corpus \
PDFP_QUALITY_OUT=/tmp/pdfp-quality \
  bash scripts/quality-report.sh
```

The report contains one entry per PDF with page count, warning count, extracted image count, empty page count, table marker count, and heading count.

Quality report mode is controlled by `PDFP_QUALITY_RECURSIVE`:

| Value | Corpus mode |
| --- | --- |
| `0`, `false`, `no` | Top-level PDFs only |
| unset, `1`, any other value | Recursive PDF traversal |

The JSON report includes:

- top-level fields: `status`, `corpus`, `corpus_mode`, `case_count`, `cases`, `quality_warnings`, `summary`
- per-case fields: `pdf`, `status`, `output`, `pages`, `warnings`, `extracted_images`, `images_per_page`, `empty_pages`, `table_markers`, `heading_count`, `heading_density`, `glued_numeric_rows`, `quality_warnings`
- warning kinds: `high_heading_density`, `high_image_density`, `glued_numeric_rows`

Compare a new quality run against a stored baseline with:

```bash
bash scripts/quality-diff.sh \
  .warden/research/local-ocr-and-quality-plan/baseline/top-level-summary.json \
  target/quality-top/report.json
```

For schema-light comparisons across quality, OmniDocBench, and table benchmark JSON outputs, use:

```bash
python tools/eval_benchmarks/compare.py old-results.json new-results.json
```

Run optional external benchmark helpers when the datasets are available locally:

```bash
python tools/eval_benchmarks/run_omni_doc_bench.py \
  --pdfp target/debug/pdfp \
  --omnidocbench /path/to/OmniDocBench \
  --output target/omnidocbench-results.json \
  --limit 10

python tools/eval_benchmarks/run_table_bench.py \
  --pdfp target/debug/pdfp \
  --benchmark rd-tablebench \
  --rd-tablebench-path /path/to/rd-tablebench \
  --output target/rd-tablebench-results.json
```

The benchmark helpers currently use only the Python standard library. `tools/eval_benchmarks/requirements.txt` is intentionally empty except for comments until a script imports a dataset-specific package directly.

Regenerate the stored baseline only after intentionally changing extraction behavior. Before regenerating, inspect the changed Markdown under the relevant `target/quality-*` case output directories.

For the repeatable improvement workflow, see the [Quality Improvement Loop](#quality-improvement-loop) section below.

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

Under ignored `test-corpus/golden/` (fixtures copied from OpenDataLoader's samples):

| File | Profile |
| --- | --- |
| `lorem.pdf` | trivial prose — fast smoke |
| `1901.03003.pdf` | arXiv layout reference |
| `2408.02509v1.pdf` | arXiv layout reference |
| `chinese_scan.pdf` | scanned Chinese document; use `--ocr auto` for local OCR or `--hybrid docling` for external assist |
| `issue-336-conto-economico-bialetti.pdf` | real-world Italian invoice |
| `pdfua-1-reference-suite-1-1/*.pdf` | 10 tagged PDF/UA-1 samples (for Phase 3 tagged-PDF verification) |

## Local OCR Setup

Local OCR is used by Markdown conversion by default through `--ocr auto`; clean born-digital PDFs skip it after scan triage. It can also be requested explicitly with:

```bash
pdfp convert scan.pdf --ocr auto --lang eng --ocr-cache-dir target/ocr-cache -o target/scan-md
pdfp ocr scan.pdf -o target/scan.searchable.pdf --mode auto --lang eng --cache-dir target/ocr-cache
pdfp inspect scan.pdf --ocr auto --json
pdfp search scan.pdf "needle" --ocr auto --json
```

`--ocr auto` first runs scan triage. Born-digital PDFs with readable text skip OCR, so missing OCR tools do not make clean PDFs fail. Scan-heavy PDFs fail with an actionable message if OCR is requested but the OCRmyPDF command is unavailable.

Runtime OCR discovery order:

1. explicit `--command <PATH>` / `--ocr-command <PATH>`
2. `PDFP_OCR_COMMAND`
3. bundled `tools/ocr/ocrmypdf` next to the installed `pdfp`
4. `ocrmypdf` from `PATH`

Install OCR dependencies before running live OCR checks:

```bash
# Arch
yay -S ocrmypdf tesseract tesseract-data-eng qpdf ghostscript
# Or use paru instead of yay.

# Debian / Ubuntu
sudo apt install ocrmypdf tesseract-ocr tesseract-ocr-eng

# macOS
brew install ocrmypdf tesseract tesseract-lang
```

Live OCR acceptance checks:

```bash
command -v ocrmypdf
command -v tesseract

target/debug/pdfp convert example/pdf/golden__chinese_scan.pdf \
  --ocr auto --lang eng --ocr-cache-dir target/ocr-cache \
  -o target/ocr-scan --verbose

target/debug/pdfp search example/pdf/golden__chinese_scan.pdf \
  "text" --ocr auto --lang eng --ocr-cache-dir target/ocr-cache --json
```

If the scan output is still only an image reference, verify that the correct Tesseract language pack is installed and try a language matching the document, for example `--lang chi_sim` for simplified Chinese scans.

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

# 4. If hybrid_live fails, run pdfp manually and inspect the raw response:
RUST_LOG=debug cargo run -- test-corpus/math-number-theory.pdf \
  --hybrid docling --hybrid-url http://localhost:5001 \
  --hybrid-policy all --hybrid-cache-dir /tmp/pdfp-docling-cache \
  --verbose -o /tmp/pdfp-live-math
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
  test-corpus/golden/pdfua-1-reference-suite-1-1/some-tagged.pdf \
  -o /tmp/pdfp-pdfium-test --verbose

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
rm -rf /tmp/pdfp-eyeball && mkdir /tmp/pdfp-eyeball
cargo run --release -- test-corpus/ -o /tmp/pdfp-eyeball --verbose
ls -la /tmp/pdfp-eyeball/*/
# then read a few of the generated .md files in your editor.
```

Things to look for:

- Two-column papers: title → authors → abstract → body in correct order, not interleaved by column.
- Math-heavy pages: if routed to Docling, display equations should appear as `$$ ... $$`. If not routed, Unicode math characters should still be present (or the page should have silently dropped any glyph without a ToUnicode map, which is the documented local-path limit — see `docs/reference/pdf-format.md` § "Fonts, encodings, and why text sometimes vanishes").
- Formula gaps: run `--debug-formulas` on standards or math fixtures. Inspect `debug/formulas/pageN.json`, matching `pageN_formulaM.png` crops, and any `formula-review` comments before treating equations as complete. If a page matters for engineering use and the local text is incomplete, rerun with `--hybrid docling --formulas hybrid` or keep the downstream standard page in draft.
- Figures: public `--images` should produce rendered page-region assets such as `![image](images/pageN_figM.png)` when a figure candidate is detected. Hidden/debug `--figures embedded` should produce `![image](images/pageN_imgM.png)` for real embedded raster figures. Snapshot mode is heuristic; inspect `--debug-figures` JSON before treating a miss as a renderer failure.
- Tables: GFM pipe tables. Missing cells are OK (the classifier's grid detector is best-effort); garbage text in cells is not OK.
- Page markers: `<!-- page:N -->` separators present, hidden on rendered display.

## Evaluation (`pdfp eval`)

`pdfp eval <fixtures-dir>` runs the local pipeline against fixture JSON files and reports recall/precision for formulas, headings, tables, figures, and decorative images.

```bash
pdfp eval tests/eval_fixtures/
```

Output shape:

```text
paper.pdf
  pages evaluated:    3
  formula recall:     75.0% (3/4)
  formula precision:  75.0% (3/4, fp 1)
  heading accuracy:   100.0% (2/2)
  heading precision:  100.0% (2/2, fp 0)
  table recall:       50.0% (1/2)
  table precision:    50.0% (1/2, fp 1)

  decorative suppression: 100.0% (0/0, emitted 0)
  meaningful figure retention: 100.0% (1/1)
  figure-caption pairing: 100.0% (1/1)
  vector-only acknowledgement: 100.0% (0/0)
```

### Fixture schema

Each `.json` file describes one PDF's expected content:

```json
{
  "pdf": "relative/path/to/file.pdf",
  "pages": [
    {
      "page": 1,
      "expected_formula_count": 2,
      "expected_formula_detection_count": 2,
      "expected_formula_latex_snippets": ["E =", "\\sqrt"],
      "formula_false_positive_budget": 0,
      "expected_headings": [{ "text": "Introduction", "level": 1 }],
      "expected_tables": 1,
      "expected_table_regions": [
        { "x0": 40.0, "y0": 100.0, "x1": 500.0, "y1": 180.0 }
      ],
      "expected_decorative_images": 0,
      "expected_meaningful_figures": 1,
      "expected_figure_captions": 1,
      "expected_vector_only_regions": 0,
      "skip_text_metrics": false,
      "skip_table_metrics": false
    }
  ]
}
```

Field notes:

- `page`: 1-indexed.
- `expected_formula_count`: total emitted formula or formula-review blocks.
- `expected_formula_detection_count`: optional candidate count checked against `debug/formulas/index.json`.
- `expected_formula_latex_snippets`: optional snippets that should appear in recovered/emitted LaTeX or source text.
- `formula_false_positive_budget`: allowed extra detected candidates before precision is penalized.
- `expected_headings`: exact text, case-insensitive and trimmed, plus heading level.
- `expected_tables`: `1` if at least one table is expected, otherwise `0`.
- `expected_table_regions`: optional expected table bboxes in page coordinates (IoU-based precision).
- `expected_decorative_images`: decorative images that should be suppressed.
- `expected_meaningful_figures`: meaningful figure regions that should be retained.
- `expected_figure_captions`: expected retained figures with paired captions.
- `expected_vector_only_regions`: vector-only regions that should be acknowledged.
- `skip_text_metrics` / `skip_table_metrics`: use for image-only benchmark pages.

`pdfp eval` reports both recall and precision. Formula precision is `matched / emitted_formula_blocks`. Heading precision is `matched / emitted_heading_blocks`. Table precision is page-based; table-region precision uses IoU against fixture bboxes. Image metrics report decorative suppression, figure retention, caption pairing, and vector-only acknowledgement.

### Adding a fixture

1. Place the PDF in this directory or use a relative path to a local corpus PDF.
2. Run `pdfp inspect <pdf>` to identify page content.
3. Create a `.json` file with expectations for the pages you want to measure.
4. Run `pdfp eval tests/eval_fixtures/`.

### Formula corpus

The tracked formula corpus under `tests/eval_fixtures/formula_corpus/` contains Typst source and generated PDFs. Regenerate with:

```sh
scripts/generate-eval-fixtures.sh formula-corpus
```

These fixtures keep formula expectations non-zero in a fresh clone without the ignored external engineering corpus.

### Stage baseline

Engineering fixtures (`engineering-calc-example.pdf`, `engineering-report-example.pdf`) live under ignored `test-corpus/eval/`. Missing PDFs are reported as skipped, keeping the JSON fixtures trackable without committing binary corpus files.

Current measured baseline:

| Signal | Result |
| --- | ---: |
| Heading accuracy | `17/21` |
| Formula recall | `13/13` |
| Table page recall | `4/4` |
| Table page precision | `4/4` |
| Table-region precision | `4/5` |
| Meaningful figure retention | `6/6` |
| Figure-caption pairing | `3/3` |
| Vector-only acknowledgement | `1/1` |

Hard image pack (generated with `scripts/generate-eval-fixtures.sh stage9-hard-images`):

| Signal | Result |
| --- | ---: |
| Decorative suppression | `1/2` |
| Meaningful figure retention | `8/9` |
| Figure-caption pairing | `5/6` |
| Vector-only acknowledgement | `1/2` |

## Quality Improvement Loop

The standard workflow for improving conversion quality:

1. Baseline (`bash scripts/example-audit.sh`).
2. Observe where output breaks down.
3. Research one failure class.
4. Change one algorithm or threshold.
5. Re-run audit and cargo checks.
6. Record the result.

The aim is not to make every PDF perfect in one pass, but to make tables, formulas, images, scans, and reading order improve with evidence.

### Sidecar comparison

```bash
bash scripts/sidecar-audit.sh
```

Runs the native `pdfp` conversion plus optional backends that skip when unavailable:

- `pdftotext-layout` — Poppler `pdftotext -layout`
- `pymupdf4llm` — PyMuPDF4LLM Python module
- `pdfplumber`, `pdfminer`, `camelot` — Python modules
- `tabula` — `PDFP_TABULA_COMMAND` or `tabula` on `PATH`
- `ocrmypdf` — OCR then native conversion
- `docling` — `PDFP_SIDECAR_DOCLING_URL` (default `http://localhost:5001`)
- `gmft`, `img2table`, `unimernet` — external command wrappers

Outputs: `target/sidecar-audit/` with per-backend directories and `summary.md`.

### Failure classes

**Tables**: strongest on simple invoices, still struggles on financial statements where row labels wrap, year columns bleed together, and subtotal rows look like equations. Next step: snapshot fixture for the Bialetti page.

**Formulas**: formula detection is an audit/escalation path, not reliable standalone LaTeX. Enrichment routes: subprocess `rapid_latex_ocr` sidecar, Docling hybrid, ONNX native (optional). Formula candidates inside high-confidence table regions are suppressed.

**Images/figures/scans**: born-digital figures are detectable; scan-heavy pages warn or need `--ocr auto`. Vector-drawn diagrams need more review than raster extraction alone.

### Change gate

Before accepting a quality change:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo check --features pdfium-metadata
bash scripts/example-audit.sh
bash scripts/sidecar-audit.sh
```

If a change is intended to improve only one class, the other classes must not regress in the audit output.

### Record template

```markdown
## YYYY-MM-DD - <change>

- Hypothesis:
- Files changed:
- Corpus:
- Before:
- After:
- Regressions:
- Next:
```

## How to add a new test

- **Unit test** — put it in a `#[cfg(test)] mod tests` block in the module under test. Reach for this first; unit tests are the fastest, most precise diagnostic.
- **Integration test** — new file under `tests/`. Use `env!("CARGO_BIN_EXE_pdfp")` to locate the binary (cargo rebuilds it automatically before running integration tests). Mark slow tests `#[ignore]` with a doc-comment explaining how to invoke them.
- **New test PDF** — drop under ignored `test-corpus/` if it's an ML paper / typical document, or `test-corpus/golden/` if it comes from an upstream fixture set. Append to `CORPUS_PATHS` in `tests/golden.rs`. If the PDF should have images, append to `EXPECTS_IMAGES` too. If it's a tagged PDF intended for Phase 3 verification, it can stay in `test-corpus/golden/pdfua-1-reference-suite-1-1/`.

## Known non-determinism

- mupdf's dominant-font-size detection bucketises to 0.5 pt, so two adjacent PDFs with 9.5 pt and 10 pt body text may classify differently depending on which bucket wins. Tests are stable because each PDF is processed in isolation.
- Temp file paths in `src/hybrid/page_extract.rs` use `std::process::id()` to avoid concurrent-test collisions, so running `cargo test -- --test-threads=1` is not required but is safer when iterating locally.
- Network errors in the live hybrid test are environment-dependent. The `hybrid_live` test is `#[ignore]` by design and is not part of CI-green criteria.
