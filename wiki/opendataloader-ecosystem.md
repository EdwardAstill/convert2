---
title: "OpenDataLoader and OpenDataLab PDF Parsing Ecosystem"
kind: "reference"
category: "wiki"
summary: "Explains how OpenDataLoader PDF, PDF-Extract-Kit, and MinerU divide the PDF parsing problem across deterministic extraction, hybrid OCR, layout analysis, and model-based reconstruction, and identifies the parts of that ecosystem that are most relevant to cnv."
virtual_path: "wiki/projects/opendataloader-ecosystem"
entities: [OpenDataLoader PDF, PDF-Extract-Kit, MinerU, XY-Cut++, DocLayout-YOLO, PaddleOCR, StructEqTable]
---

# OpenDataLoader and OpenDataLab PDF Parsing Ecosystem

The OpenDataLoader and OpenDataLab ecosystem is one of the most useful external references for `cnv` because it does not treat PDF parsing as one problem. It separates deterministic structure recovery, hybrid escalation for hard pages, and model-level extraction tasks such as OCR, formula recovery, and complex table reconstruction.

## OpenDataLoader PDF Architecture

`OpenDataLoader PDF` is the closest external project to `cnv`'s current direction. It uses a deterministic local parser with bounding boxes, XY-Cut++ reading order, and explicit support for Tagged PDF extraction, then escalates only the hard pages into a hybrid path.

Key implementation traits:

- Java core with Python, Node.js, and Java SDKs
- deterministic CPU-first extraction in local mode
- bounding boxes for every element
- XY-Cut++ reading order for multi-column and mixed-layout PDFs
- Tagged PDF structure support
- hybrid mode for OCR, complex tables, formulas, charts, and images
- explicit hidden-text and off-page filtering for prompt-injection safety

The strongest lessons for `cnv` are:

- make per-page routing explicit
- preserve element boxes internally even if Markdown is the only active top-level export
- support tagged-PDF structure when present
- add invisible-text and off-page filtering as both a quality feature and a safety feature

## PDF-Extract-Kit Architecture

`PDF-Extract-Kit` is not a direct PDF-to-Markdown converter. It is a modular model toolkit for the subproblems that deterministic parsers struggle with.

Its modules include:

- layout detection
- formula detection
- formula recognition
- OCR
- table recognition

The published stack currently includes:

- `DocLayout-YOLO`, `YOLO-v10_ft`, and `LayoutLMv3_ft` for layout detection
- `PaddleOCR` for OCR
- `UniMERNet` for formula recognition
- `PaddleOCR + TableMaster`, `StructEqTable`, and `StructTable-InternVL2-1B` for table recognition

The most important lesson for `cnv` is architectural, not model-specific:

- layout detection, OCR, formula recognition, and table recognition should be separable modules with different triggers, different benchmarks, and different debugging tools

## MinerU Architecture

`MinerU` sits closer to the end-user conversion layer. It converts PDFs and other complex document types into Markdown and JSON using a dual-engine VLM + OCR pipeline.

Key implementation traits:

- Python stack
- VLM + OCR dual engine
- 109-language OCR support
- multi-column support
- header and footer removal
- cross-page table merging
- formula-to-LaTeX output
- table-to-HTML output

The most relevant lessons for `cnv` are:

- cross-page table continuation deserves explicit handling
- header and footer suppression is worth treating as a first-class pass
- financial and report-style documents need a more document-aware parser than generic prose pages

## OpenDataLoader Versus OpenDataLab

The naming is confusing, but the practical split is useful:

- `OpenDataLoader PDF` is a deterministic/hybrid document parser aimed directly at Markdown and JSON conversion
- `PDF-Extract-Kit` is a lower-level model toolkit for layout, OCR, formula, and table tasks
- `MinerU` is a higher-level document conversion engine using more model-heavy parsing

For `cnv`, that suggests a three-layer way of thinking:

1. deterministic local parser first
2. specialized recovery passes second
3. optional OCR or heavier escalation only when confidence is low

## What `cnv` Should Borrow from the OpenDataLoader Ecosystem

The highest-value ideas to borrow are:

1. **Tagged PDF support**
   `OpenDataLoader PDF` treats structure tags as authoritative. `cnv` should do the same when `/StructTreeRoot` data is available.

2. **Explicit page routing**
   `OpenDataLoader PDF` makes the distinction between fast local mode and hybrid mode very clear. `cnv` should continue moving toward page- and region-level routing instead of whole-document guessing.

3. **Bounding-box-rich internal state**
   Even if `cnv` only emits Markdown, keeping richer geometric structure internally makes tables, forms, captions, and debug overlays much easier to improve.

4. **Hidden-text safety filtering**
   Invisible or off-page text filtering is useful both for Markdown quality and for prompt-injection resistance in downstream LLM workflows.

5. **Modular table and formula handling**
   `PDF-Extract-Kit` makes it obvious that tables and formulas need specialized treatment. `cnv` should keep hard-table parsing separate from generic paragraph rendering.

6. **Cross-page table continuity**
   `MinerU` treats cross-page structure as a real problem. `cnv` should explicitly merge continuation tables rather than handling each page in isolation.

## What `cnv` Should Not Copy Blindly

`cnv` should not copy:

- GPU-heavy default pipelines
- hosted-service assumptions
- VLM-first conversion as the main path
- broad multi-format scope at the expense of PDF depth

`cnv` should copy:

- pipeline decomposition
- routing logic
- explicit document classes
- table and scan confidence signals
- debug and evaluation discipline

## Sources for the OpenDataLoader and OpenDataLab Ecosystem

- `OpenDataLoader PDF`: <https://github.com/opendataloader-project/opendataloader-pdf>
- `PDF-Extract-Kit`: <https://github.com/opendatalab/PDF-Extract-Kit>
- `MinerU`: <https://github.com/opendatalab/MinerU>
