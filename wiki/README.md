# PDF-to-Markdown Wiki

This wiki is a working knowledge base for building and improving `cnv`.

It covers:

- how PDFs are structured
- how text, layout, tables, images, and scans should be handled
- how to turn those pieces into useful Markdown
- how to extract structured information from PDFs after conversion
- what strong open-source projects are doing well
- what `cnv` should build next

This is not a general-purpose PDF encyclopedia. It is scoped to the practical problems that matter for local, code-first PDF-to-Markdown conversion.

## Start here

- [Pipeline Overview](pipeline-overview.md)
- [Text Extraction](text-extraction.md)
- [Layout and Reading Order](layout-and-reading-order.md)
- [Tables, Forms, and Financial Documents](tables-forms-and-financials.md)
- [Scans and OCR](scans-and-ocr.md)
- [Markdown Rendering](markdown-rendering.md)
- [Information Extraction](information-extraction.md)
- [Evaluation and Benchmarks](evaluation-and-benchmarks.md)
- [Reference Implementations](reference-implementations.md)
- [Project Comparison Matrix](project-comparison-matrix.md)
- [OpenDataLoader Ecosystem](opendataloader-ecosystem.md)
- [Improvement Opportunities](improvement-opportunities.md)

## Existing repo documents worth reading

- [Top-level README](../README.md) for install, CLI usage, and current scope
- [PDF internals](../docs/pdf-internals.md) for low-level object/content-stream details
- [Testing guide](../docs/TESTING.md) for the current verification matrix
- [Example assessment](../example/ASSESSMENT.md) for current quality across the example corpus
- [Fix plan](../example/FIX_PLAN.md) for the current implementation roadmap
- [Research notes](../example/RESEARCH_NOTES.md) for GitHub-based implementation takeaways

## Current design position

`cnv` is intentionally:

- PDF-first
- Markdown-first
- local-first
- code-first

That means:

- the core converter should not depend on hosted APIs
- deterministic extraction and heuristics come first
- OCR is acceptable as a local preprocessing step for scans
- models can inform research and benchmarking, but should not silently become the default conversion path

## Current biggest gaps

Based on the example corpus:

1. financial statement reconstruction
2. scan-heavy PDFs without a local OCR path
3. magazine and brochure layout grouping
4. business header and key-value normalization
5. broader scholarly first-page generalization

If you only read three pages in this wiki, read:

1. [Pipeline Overview](pipeline-overview.md)
2. [Tables, Forms, and Financial Documents](tables-forms-and-financials.md)
3. [Scans and OCR](scans-and-ocr.md)

If you want to understand the external landscape first, read:

1. [Project Comparison Matrix](project-comparison-matrix.md)
2. [OpenDataLoader Ecosystem](opendataloader-ecosystem.md)
3. [Improvement Opportunities](improvement-opportunities.md)
