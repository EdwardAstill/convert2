---
title: "PDF-to-Markdown Project Comparison Matrix"
kind: "reference"
category: "wiki"
summary: "Compares major open-source PDF-to-Markdown and document parsing projects by language, core stack, OCR path, layout strategy, table strategy, and the implementation lessons most relevant to cnv."
virtual_path: "wiki/projects/project-comparison-matrix"
entities: [OpenDataLoader PDF, pdf-inspector, Docling, Marker, PDF-Extract-Kit, MinerU, PyMuPDF4LLM, pdfplumber, MarkItDown]
---

# PDF-to-Markdown Project Comparison Matrix

The current open-source landscape splits into two broad camps: deterministic local parsers that recover structure from PDF primitives, and model-heavy parsers that use OCR, layout models, VLMs, or LLMs to recover difficult pages. The most useful implementation work for `cnv` comes from understanding where each project sits on that spectrum and stealing the parts that solve `cnv`'s real failure cases.

## Comparison of Similar PDF-to-Markdown Projects

| Project | Primary language | Core stack | OCR strategy | Layout and reading-order strategy | Table strategy | Most useful lesson for `cnv` |
| --- | --- | --- | --- | --- | --- | --- |
| `opendataloader-pdf` | Java core, Python/Node/Java SDKs | deterministic local parser plus optional local hybrid backend | built-in OCR in hybrid mode | XY-Cut++ for reading order, bounding boxes for every element, struct-tree support | border analysis and text clustering locally; hybrid mode for complex tables | make document triage explicit, support tagged PDFs, and treat hybrid/OCR as a page-level escalation path |
| `firecrawl/pdf-inspector` | Rust | `lopdf`, direct content-stream walking, one-pass document load | none; instead classifies pages needing OCR | position-aware extraction, multi-column ordering, per-page classification | rectangle-based detection from drawing ops plus heuristic alignment tables | build a clearer table pipeline and expose scan/encoding confidence as first-class signals |
| `docling` | Python | unified `DoclingDocument`, OCR, layout models, export adapters | first-class OCR support for scanned PDFs and images | advanced PDF understanding with reading order, table structure, code, formulas, image classification | structured table understanding inside a richer document model | build a stronger intermediate representation and treat financial/XBRL-style docs as a distinct class |
| `marker` | Python | PyTorch, Surya OCR/layout, optional LLM post-processing | OCR on demand or forced OCR | Surya-based layout and reading-order recovery, block processors | separate `PdfConverter` and `TableConverter`, optional LLM cleanup and table merging | add a table-specialized conversion path and keep debug artifacts for page layout and tables |
| `PDF-Extract-Kit` | Python | modular model toolkit | `PaddleOCR` | model-based layout detection; reading order listed separately | `TableMaster`, `StructEqTable`, `StructTable-InternVL2-1B` | split layout detection, OCR, formulas, and table recognition into explicit modules even if `cnv` stays code-first |
| `MinerU` | Python | VLM + OCR dual engine, multi-format parser | 109-language OCR in hybrid/VLM pipelines | human reading order with header/footer removal and multi-column support | HTML tables, cross-page table merging, complex layout reconstruction | invest in cross-page table continuation and stronger header/footer suppression |
| `PyMuPDF4LLM` | Python | MuPDF engine plus layout helpers | selective OCR only where needed | layout-aware reading order with multi-column support | built-in table detection to GitHub Markdown | OCR should be selective and confidence-driven, not a blanket pass over every page |
| `pdfplumber` | Python | `pdfminer.six`, page primitive inspection, visual debugging | none | low-level object access; user-directed layout tuning | line-, text-, or explicit-strategy table extraction with intersection-based cells | debugability matters; expose block, line, edge, and intersection views to tune heuristics quickly |
| `MarkItDown` | Python | lightweight converter plus plugins | OCR only via plugin, often LLM-vision-based | broad file-format support, structure preservation as Markdown | generic table/list preservation, plugin-based extension | plugin architecture is useful, but broad multi-format scope tends to weaken PDF-specific quality |

## Comparison of Deterministic and Model-Heavy Approaches

Deterministic parsers such as `opendataloader-pdf`, `pdf-inspector`, `PyMuPDF4LLM`, and `pdfplumber` are strongest when the PDF already contains a decent text layer. They are faster, easier to debug, cheaper to run locally, and much better aligned with `cnv`'s code-first direction.

Model-heavy parsers such as `docling`, `marker`, `PDF-Extract-Kit`, and `MinerU` are strongest on:

- scans
- formulas
- visually-defined tables
- charts and image understanding
- documents where semantic structure is not recoverable from text geometry alone

The useful conclusion is not "turn `cnv` into a model stack." The useful conclusion is:

- deterministic parsing should remain the default
- difficult pages need explicit escalation paths
- the escalation trigger should be based on real signals such as scan density, broken encodings, table complexity, or missing structure

## Comparison of Core Algorithms and Heuristics

The strongest recurring implementation patterns across these projects are:

- **XY-Cut or equivalent reading-order recovery** for multi-column and mixed-layout pages
- **font-size tiering** for headings, often improved by font metadata or document TOC/tag information
- **table detection from both geometry and text alignment** rather than from text alone
- **page or region classification before conversion** instead of one monolithic parser path
- **local OCR or hybrid OCR escalation** when text extraction confidence is low
- **debug overlays and artifact dumps** for lines, cells, tables, and block ordering
- **intermediate document models** that delay Markdown emission until structure is stable

## Comparison of Tooling and Dependencies

These projects also suggest practical tooling choices:

- Rust projects tend to stay light and deterministic: `pdf-inspector`
- Java is still competitive for PDF structure and tagged-PDF work: `opendataloader-pdf`
- Python dominates model-heavy parsing because the OCR, layout, and VLM ecosystems live there: `docling`, `marker`, `MinerU`, `PDF-Extract-Kit`
- MuPDF-backed projects are attractive when speed and local execution matter: `PyMuPDF4LLM`
- `pdfminer.six`/`pdfplumber` remain valuable as debugging and primitive-inspection references even if they are not the final converter architecture

## Lessons for `cnv` from the Project Comparison

The comparison points toward a practical roadmap for `cnv`:

1. Keep the default path deterministic and local.
2. Make page triage more explicit and confidence-aware.
3. Build a dedicated numeric-heavy table pass.
4. Add a local OCR preprocessing path for scan-heavy pages.
5. Read tagged PDF structure when available instead of always guessing.
6. Add debug artifacts for table and reading-order decisions.
7. Separate structure recovery from Markdown rendering more aggressively.

## Sources for the Project Comparison

- `opendataloader-pdf`: <https://github.com/opendataloader-project/opendataloader-pdf>
- `firecrawl/pdf-inspector`: <https://github.com/firecrawl/pdf-inspector>
- `docling`: <https://github.com/docling-project/docling>
- `marker`: <https://github.com/datalab-to/marker>
- `PDF-Extract-Kit`: <https://github.com/opendatalab/PDF-Extract-Kit>
- `MinerU`: <https://github.com/opendatalab/MinerU>
- `PyMuPDF4LLM`: <https://github.com/pymupdf/pymupdf4llm>
- `pdfplumber`: <https://github.com/jsvine/pdfplumber>
- `MarkItDown`: <https://github.com/microsoft/markitdown>
