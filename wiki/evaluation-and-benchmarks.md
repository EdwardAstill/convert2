# Evaluation and Benchmarks

A PDF converter only gets better if it is tested against real failure shapes.

## The minimum viable evaluation loop

Use this loop:

1. run the converter on a standing corpus
2. inspect the Markdown
3. identify concrete failure patterns
4. add targeted fixes
5. add regression tests
6. rerun the corpus
7. update assessment and plan

This is the loop already being used in this repo.

## Why one metric is not enough

PDF conversion quality is multi-dimensional.

Different slices need different checks:

- prose readability
- reading order
- heading hierarchy
- table structure
- form usability
- scan recoverability

## The benchmark set in this repo

Current standing set includes:

- scholarly papers
- invoices
- forms
- financial statements
- magazines and brochures
- scan-heavy PDFs

See:

- [Example assessment](../example/ASSESSMENT.md)
- [Fix plan](../example/FIX_PLAN.md)

## What to measure

### Human-readable Markdown quality

Questions:

- would a person actually want to read this?
- are headings useful?
- are paragraphs coherent?
- is noise under control?

### Table quality

Questions:

- are rows preserved?
- are columns preserved?
- are headers identified?
- are totals still meaningful?

### Scan quality

Questions:

- is the output usable at all?
- does the tool correctly identify scan-heavy cases?
- does OCR materially improve the result?

## Recommended evaluation additions

### Table-specific fixtures

Add expected structured fixtures for hard table regions:

- row count
- column count
- cell text
- header/body distinction

This will help much more than a pure Markdown snapshot for financial work.

### Debug artifacts

Keep optional debug outputs for:

- block boxes
- reading order
- candidate columns
- table regions

These make it easier to understand why a regression happened.

### Document-class dashboards

Track progress by document class:

- scholarly
- business
- financial
- layout-heavy
- scans

This avoids false confidence from improvements that only help one class.

## Good local truth sources

When evaluating difficult PDFs, compare against:

- the source PDF
- `pdftotext`
- current `cnv` Markdown
- historical snapshots

## Related pages

- [Pipeline Overview](pipeline-overview.md)
- [Reference Implementations](reference-implementations.md)
