# Research Notes

This note captures useful ideas from current open-source PDF projects and maps them onto `cnv`'s actual failure modes.

The goal here is **not** to turn `cnv` into a model-first tool. The goal is to steal good engineering ideas, evaluation methods, and pipeline structure from projects that are already strong on the cases where `cnv` is still weak.

## Most useful outside references

### 1. `firecrawl/pdf-inspector`

Repository: <https://github.com/firecrawl/pdf-inspector>

Why it matters:

- It is a Rust-first, local, code-heavy project with a very similar shape to `cnv`.
- Its README is unusually explicit about architecture and tradeoffs.
- It separates:
  - PDF classification
  - text extraction
  - layout analysis
  - table detection
  - markdown formatting

Useful ideas:

- stronger per-page document triage instead of one coarse document-level decision
- explicit PDF types: text-based, scanned, image-based, mixed
- clearer internal pipeline boundaries
- debug-friendly architecture for tables:
  - rectangle-based tables
  - alignment-based tables
  - grid assignment
  - markdown formatting

Why this is relevant to `cnv`:

- `cnv` is already closest to this family of tool
- scan-heavy routing and table detection can become more explicit and inspectable
- the repo also openly notes that heading detection and visually defined tables are hard, which matches our own findings

### 2. `docling`

Repository: <https://github.com/docling-project/docling>

Why it matters:

- it covers several problem classes `cnv` still struggles with:
  - table structure
  - OCR support
  - reading order
  - financial reports / XBRL
- even when we do not copy the implementation, the feature split is useful

Useful ideas:

- treat financial reports as a distinct document class, not just "another table"
- use a richer intermediate document model before markdown rendering
- keep OCR as a first-class path, not an afterthought

Why this is relevant to `cnv`:

- our Italian financial statement failures are not generic text failures
- they are document-class-specific structural failures
- this argues for a dedicated financial parser mode rather than piling more generic regex onto the renderer

### 3. `marker`

Repository: <https://github.com/datalab-to/marker>

Why it matters:

- the repo exposes a clean split between whole-document conversion and table-only conversion
- it also documents its limitations clearly enough to be useful for our planning

Useful ideas:

- separate "whole PDF" conversion from "table-first" conversion
- allow forcing a page or region into table mode when signals are strong
- benchmark table quality independently from full-document markdown quality

Why this is relevant to `cnv`:

- our current worst failures are concentrated in financial tables
- a table-specialized pass could improve those pages without destabilizing strong paper output

### 4. `microsoft/table-transformer`

Repository: <https://github.com/microsoft/table-transformer>

Why it matters:

- it provides a clean decomposition of table work:
  - table detection
  - table structure recognition
  - functional analysis
- it also ships the PubTables-1M dataset and GriTS evaluation metric

Useful ideas:

- explicitly split table work into:
  - detect table region
  - infer rows/columns/cells
  - identify header cells vs body cells
- produce debug visualizations during development
- evaluate table quality with a dedicated metric instead of only eyeballing markdown

Why this is relevant to `cnv`:

- our financial doc problem is mostly a structure-recognition problem
- our current tests prove regressions, but they do not measure table structure quality very well

### 5. `OCRmyPDF`

Repository: <https://github.com/ocrmypdf/ocrmypdf>

Why it matters:

- it solves the exact local-scan problem `cnv` still has
- it does so in a practical way by adding a searchable text layer back into the PDF
- it also supports alternate OCR backends

Useful ideas:

- treat OCR as a preprocessing step that improves the PDF itself
- preserve original PDF while writing a derived searchable PDF
- keep OCR local and optional

Why this is relevant to `cnv`:

- `golden__chinese_scan.pdf` is still poor in the current local path
- the cleanest code-first fix is likely "preprocess scan into searchable PDF, then reuse existing extraction path"

### 6. `PDF-Extract-Kit`

Repository: <https://github.com/opendatalab/PDF-Extract-Kit>

Why it matters:

- its README reflects a modular decomposition that is helpful even if we do not adopt the models:
  - layout detection
  - OCR
  - table recognition
  - formula detection
  - reading order
- it explicitly calls out robustness on financial reports as a target

Useful ideas:

- make `cnv`'s pipeline modules more explicit by failure type
- isolate table recognition from generic page reading order
- build evaluation pages for financial-report robustness

## Practical ideas worth stealing now

These are the ideas that fit `cnv`'s current code-first direction.

## A. Add a table-specialized pass for numeric-heavy regions

Inspired by:

- `pdf-inspector` table pipeline
- `marker` table-only conversion mode
- `table-transformer` detect-then-structure split

Concrete plan:

1. Detect candidate table regions or page spans using:
   - repeated right-aligned numeric columns
   - consistent baseline spacing
   - long left labels plus 3-6 numeric values
2. Run a dedicated table parser on those regions.
3. Only fall back to generic paragraph rendering if the table parser has low confidence.

Why this should help:

- it directly targets the remaining Bialetti failures
- it avoids overfitting generic markdown rendering to one hard class of page

## B. Add per-page and per-region confidence/debug artifacts

Inspired by:

- `table-transformer` debug outputs
- `pdf-inspector` explicit architecture and table stages

Concrete plan:

1. Add an internal debug mode that emits:
   - detected text blocks
   - candidate table columns
   - chosen reading order
   - merged/suppressed furniture blocks
2. Write artifacts per page into a debug folder.
3. Use these during regression work on magazine and financial docs.

Why this should help:

- right now some failures are hard to diagnose from markdown alone
- visual/debug artifacts would make heuristic tuning much faster

## C. Promote financial statements to a first-class document subtype

Inspired by:

- `docling` support for financial reports / XBRL
- `PDF-Extract-Kit` emphasis on robust diverse financial parsing

Concrete plan:

1. Detect financial statements using lexical cues:
   - repeated accounting section markers
   - dense numeric columns
   - balance sheet / income statement vocabulary
2. Switch to a financial renderer/parser mode.
3. In that mode:
   - prefer multi-page table continuity
   - preserve section/subsection nesting
   - merge adjacent fragments into one logical table

Why this should help:

- it matches the real structure of the hard examples
- it gives us somewhere clean to put financial-specific heuristics

## D. Add a local OCR preprocessing mode for scan-heavy PDFs

Inspired by:

- `OCRmyPDF`
- scan routing ideas from `pdf-inspector`

Concrete plan:

1. Add optional `--ocr` or `--ocr-command` support.
2. For scan-heavy PDFs:
   - generate a searchable derived PDF in a temp/work dir
   - rerun the normal extraction pipeline on that derived PDF
3. Keep the current warning path when OCR is unavailable.

Why this should help:

- this is the shortest path from "scan warning" to "scan usable"
- it preserves the rest of the pipeline instead of forking conversion logic

## E. Add a proper table-evaluation benchmark track

Inspired by:

- `marker` FinTabNet benchmarking
- `table-transformer` GriTS / PubTables-1M

Concrete plan:

1. Keep current human-readable markdown snapshots.
2. Add a small table benchmark set with expected CSV/JSON cell grids.
3. Score:
   - row count correctness
   - column count correctness
   - cell text correctness
   - header/body correctness

Why this should help:

- current snapshots are good for regressions but weak for measuring partial table improvements
- financial work needs tighter feedback than "looks a bit better"

## F. Separate "page reading order" work from "block rendering" work

Inspired by:

- `docling`
- `PDF-Extract-Kit`
- `pdf-inspector`

Concrete plan:

1. Introduce a richer intermediate representation for:
   - paragraph
   - heading
   - table
   - form field
   - page furniture
   - image
2. Do markdown rendering only after structure is stable.

Why this should help:

- several current fixes are accumulating inside markdown rendering heuristics
- this is becoming harder to reason about for magazine and business docs

## Ideas to study, but not adopt blindly

These look useful, but should not become default direction without care.

### Model-based layout/table recognition

Projects like `docling`, `marker`, `table-transformer`, and `PDF-Extract-Kit` are strong partly because they use learned models.

That is useful as research input, but it does **not** mean `cnv` should pivot away from code-first conversion. The immediate value for `cnv` is:

- stealing pipeline decomposition
- stealing evaluation methods
- stealing document-type distinctions
- stealing debug approaches

not:

- adding a silent model dependency to the core converter

### LLM-assisted table cleanup

`marker` shows better table scores with LLM help. That is interesting as a benchmark signal, but it is not aligned with the current direction for `cnv`.

The better takeaway is:

- our hard tables need a dedicated structure stage
- current generic rendering is leaving quality on the table

## Recommended next roadmap

Based on both the current example outputs and the GitHub research:

1. Build a dedicated financial/table pass for numeric-heavy regions.
2. Add debug artifacts for block, column, and table decisions.
3. Promote financial statements to a first-class document subtype.
4. Add local OCR preprocessing for scan-heavy PDFs.
5. Add table-specific evaluation fixtures and scoring.
6. Refactor toward a richer intermediate structure before markdown rendering.

## Sources

- `firecrawl/pdf-inspector`: <https://github.com/firecrawl/pdf-inspector>
- `docling`: <https://github.com/docling-project/docling>
- `marker`: <https://github.com/datalab-to/marker>
- `microsoft/table-transformer`: <https://github.com/microsoft/table-transformer>
- `OCRmyPDF`: <https://github.com/ocrmypdf/ocrmypdf>
- `PDF-Extract-Kit`: <https://github.com/opendatalab/PDF-Extract-Kit>
