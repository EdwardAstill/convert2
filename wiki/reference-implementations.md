# Reference Implementations

This page captures the most useful implementation takeaways from current open-source PDF projects. The detailed comparison now lives in two supporting pages:

- [Project Comparison Matrix](project-comparison-matrix.md)
- [OpenDataLoader Ecosystem](opendataloader-ecosystem.md)

The goal is not to copy them wholesale. The goal is to extract practical ideas that can improve `cnv`.

## `firecrawl/pdf-inspector`

Repository: <https://github.com/firecrawl/pdf-inspector>

Why it matters:

- Rust-first
- local-first
- explicit PDF classification
- explicit table pipeline

Takeaways worth copying:

- stronger per-page triage
- explicit document classes like text-based, scanned, image-based, and mixed
- clearer boundaries between extraction, layout analysis, tables, and Markdown formatting
- debug-friendly table stages

Best fit for `cnv`:

- triage improvements
- table pipeline cleanup
- scan-routing design

## `docling`

Repository: <https://github.com/docling-project/docling>

Why it matters:

- broad document understanding
- strong handling of tables, reading order, OCR, and financial reports
- richer intermediate document model

Takeaways worth copying:

- financial documents should be treated as a distinct class
- OCR should be a first-class path, not a bolted-on afterthought
- a richer internal representation makes export more robust

Best fit for `cnv`:

- financial subtype detection
- intermediate representation design
- scan strategy

## `marker`

Repository: <https://github.com/datalab-to/marker>

Why it matters:

- clearly separates full-document conversion from table-focused conversion
- documents table benchmarking and limitations

Takeaways worth copying:

- support table-specialized processing for pages or regions when signals are strong
- benchmark table quality separately from overall document quality
- keep whole-document conversion and table conversion conceptually distinct

Best fit for `cnv`:

- numeric-heavy table pass
- better evaluation of financial documents

## `microsoft/table-transformer`

Repository: <https://github.com/microsoft/table-transformer>

Why it matters:

- clean decomposition of table detection and table structure recognition
- useful evaluation framing around table structure quality

Takeaways worth copying:

- split table work into:
  - table detection
  - row/column/cell reconstruction
  - header/body understanding
- emit debug visualizations during development
- use table-specific evaluation, not only Markdown eyeballing

Best fit for `cnv`:

- financial and table reconstruction
- debug artifact design
- benchmark design

## `OCRmyPDF`

Repository: <https://github.com/ocrmypdf/ocrmypdf>

Why it matters:

- practical local OCR path
- creates searchable PDFs that downstream tools can reuse

Takeaways worth copying:

- OCR should be a preprocessing step for scans
- keep the original file intact
- generate a searchable derivative and reuse the normal pipeline

Best fit for `cnv`:

- local OCR preprocessing mode

## `PDF-Extract-Kit`

Repository: <https://github.com/opendatalab/PDF-Extract-Kit>

Why it matters:

- good modular decomposition of layout, OCR, table recognition, and reading order
- explicitly targets hard financial-style documents

Takeaways worth copying:

- separate layout detection from table recognition
- isolate failure classes
- maintain a modular pipeline instead of a single monolithic extraction stage

Best fit for `cnv`:

- architecture cleanup
- financial/table work

## What to copy and what not to copy

Copy:

- pipeline boundaries
- debug and evaluation ideas
- document-class distinctions
- local OCR strategy

Do not copy blindly:

- hosted-service assumptions
- GPU-heavy default paths
- model-first conversion as the only solution

## Repo-local follow-up

For the current prioritized plan, see:

- [Research notes](../example/RESEARCH_NOTES.md)
- [Fix plan](../example/FIX_PLAN.md)
- [Improvement opportunities](improvement-opportunities.md)
