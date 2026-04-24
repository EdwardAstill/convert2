# `cnv` — architecture

This document describes `cnv`'s internals in depth. For an introduction to the repo's purpose see `README.md`. For the underlying PDF format concepts see `docs/pdf-internals.md`. For the implementation roadmap see `docs/plans/2026-04-20-opendataloader-parity.md`.

## Pipelines at a glance

`cnv` has three pipeline families, dispatched from `main.rs::process_one()` by input file extension.

```
          ┌───────────────────────────────────────────┐
          │           File input (CLI arg)            │
          └───────────────────┬───────────────────────┘
                              │  ext-based dispatch
         ┌────────────────────┼────────────────────┐
         │                    │                    │
   ┌─────▼──────┐      ┌──────▼──────┐      ┌──────▼──────┐
   │ PIPELINE A │      │ PIPELINE B  │      │ PIPELINE C  │
   │ Document   │      │ Markdown →  │      │ SVG → PNG   │
   │ → Markdown │      │ Typst       │      │             │
   └─────┬──────┘      └──────┬──────┘      └──────┬──────┘
         │                    │                    │
         │              .typ file            .png file
         │
   ┌─────▼────────────────────────────────────────────────┐
   │ Extract → layout → classify → render → format writer │
   └──────────────────────────────────────────────────────┘
```

Pipeline A is where all the interesting work happens. Pipelines B and C are small, standalone, and described at the end.

## Pipeline A — Document to Markdown

### Top-level dispatch

```
InputType::from_path(&path)          // by extension
  → PdfExtractor::extract | docx::… | epub::… | pptx::… | html_extract::…
    → Vec<RawPage>  (for PDF) or Document (for the rest)
      → [PDF only] build_xycut_order → assign_reading_order → Classifier::classify_page
        → Document
          → MarkdownRenderer::render_document
            → RenderedDocument
              → formats::{raw|rag|karpathy|kg|json}::write
```

For PDFs only, there is a parallel hybrid path. When `--hybrid docling` is on, `process_pdf` short-circuits to `process_pdf_hybrid`, which calls `hybrid::convert_pdf_markdown`, synthesises a minimal `RenderedDocument` from the returned markdown, and hands that to the format writer directly. The local pipeline is skipped in hybrid mode.

### PDF pipeline — detailed

```
PdfExtractor::extract(&path) -> (Vec<RawPage>, DocumentMetadata)

  For each page:
    mupdf TextPage with PRESERVE_IMAGES
      → iterate blocks
          - Text blocks → collect_block_text → RawTextBlock {bbox, text, font_size}
          - Image blocks → decode to PNG bytes → ImageRef {bbox, bytes, format}
          - Vector / Grid / Struct blocks → dropped

classifier = Classifier::new_for_document(&raw_pages)
  // computes document-wide body font size (mode over 0.5pt buckets)

For each raw_page:
  order = build_xycut_order(&raw_page.blocks, &config)     // XY-Cut++
  assign_reading_order(&order, &mut raw_page.blocks)

  text_classified = classifier.classify_page(text_blocks, &page_shell)
  // Font-size + regex-based classification into BlockKind variants

  image_blocks = save_page_images(&image_refs, &images_dir)
  // Decode PNGs, emit BlockKind::Image { path: Some(...) }

  page.blocks = merge_text_and_images(text_classified, image_blocks)
  // Interleave by bbox.y0, re-assign reading_order

Document { pages, metadata, source_path }
```

### Layout analysis — XY-Cut++

`src/layout/xycut.rs` ports OpenDataLoader's Java implementation (Hancom, Apache-2.0) of the XY-Cut++ algorithm from arXiv:2504.10258. Four phases:

1. **Cross-layout pre-mask.** Identify wide elements (width ≥ β × max_width) with ≥ 2 horizontal overlaps. These are titles, full-width figures, spanning section headers. With default β = 2.0 (Java parity), this is effectively disabled — nothing is ≥ 2× the max. Callers can lower β to activate pre-masking.
2. **Density ratio.** Computed as content_area / bounding_region_area but not currently consulted as a decision driver; reserved for future paper-mode parity.
3. **Recursive segmentation.** At each step, find the largest Y-axis gap and the largest X-axis gap. Take the axis with the larger gap, provided it exceeds `min_horizontal_gap` or `min_vertical_gap` respectively. Narrow-outlier retry: when the X-axis gap is too small, filter items whose width < 10 % of region width and re-scan — this rescues two-column pages where a page number bridges the gutter.
4. **Merge cross-layout back.** Sort the pre-masked elements by y0 and interleave into the main stream by a simple Y-merge. Ties go to cross-layout.

Coordinate convention: `cnv` uses top-left origin (y grows downward). The Java source uses PDF-native y-up. The module's top-of-file doc-comment carries the flip table.

### Classification

`src/layout/classifier.rs` assigns a `BlockKind` to each `RawTextBlock` after reading order is set. It uses a priority-ordered rule cascade:

1. Page header zone (`y1 ≤ 0.07 × page_height`) → `PageNumber` (if matches digit regex) or `RunningHeader`.
2. Page footer zone (`y0 ≥ 0.93 × page_height`) → same for footer.
3. Caption regex: `^(figure|fig\.?|table|tbl\.?|algorithm|listing|exhibit)\s+[\dIVXivx]+[.:)]`.
4. Code-block regex (heuristic, fragile).
5. Ordered-list regex: `\d+[.)]` or `[a-zA-Z][.)]`.
6. Unordered-list regex.
7. Font-size ratio vs document body size: ≥ 1.15 → heading, with level bucketed from ratio thresholds.
8. Default: `Paragraph`.

Table detection is a separate pre-pass: `detect_table_cells_with_font_size` clusters blocks into grid rows (y-clustering), filters by a non-table-candidate list, then clusters rows into candidate regions, then runs `detect_table_in_region` to determine columns (x-clustering) and produce `BlockKind::TableCell { row, col }`.

Brittle spots, for the next contributor:

- Font-size-only heading detection fails on documents with uniform body font. Phase 3 of the plan (add `pdfium-render` for font names + struct tree) is the remedy.
- The code-block regex (`^\s*(\`\`\`|~~~|def |fn |pub |class |import |from |#include|int |void |return )`) matches any paragraph starting with `return ` — prose gets misclassified as code occasionally.
- Table x-clustering uses `bbox.x0` (left edge). Right-aligned numeric columns vary their left edges by digit count, so they sometimes split into multiple clusters.

### Rendering

`src/render/markdown.rs` walks `Document.pages[*].blocks` sorted by `reading_order` and emits markdown. Contiguous runs of `ListItem` and `TableCell` blocks are collected and rendered as a single list or table. `Image`, `Figure`, `Formula` each have their own serialization. `PageNumber`, `RunningHeader`, `RunningFooter` are silently dropped — the classifier already marked them as navigation chrome.

Output is a single `markdown: String` plus `sections: Vec<Section>` (split on heading boundaries) plus `images: Vec<ExtractedImage>` (currently populated only by non-PDF input types) plus `source_path`.

### Format writers

Format writers (`src/formats/{raw,rag,karpathy,kg,json}/mod.rs`) consume `RenderedDocument` + `Document` and write whatever output shape they want:

- **raw** — a single `<stem>.md` file.
- **rag** — split into ~N-token chunks, one `<stem>-chunk-<i>.md` per chunk.
- **karpathy** — wiki-style folder, one file per section, linked by `[[wikilinks]]`.
- **kg** — knowledge-graph JSON: citations + cross-references extracted.
- **json** — structured JSON mirroring a subset of the Docling / OpenDataLoader document schema. Best for downstream programmatic consumers.

Each writer knows how to lay itself out under `<output>/<stem>/`. Images land in `<output>/<stem>/images/` and are already on disk by the time the writer runs (saved during `process_pdf`).

## Pipeline B — Markdown to Typst

```
<input>.md
  → pulldown-cmark event stream
    → typst::converter::convert (event-driven walker)
       └── latex_to_typst() whenever math is encountered
  → <output>.typ
```

Config sources at `$XDG_CONFIG_HOME/mdtyp/config.toml` or `--typst-config`. The LaTeX → Typst math translator is a 5-stage pipeline in `src/typst/latex2typst.rs`: environments → structured commands → simple replacements → scripts → identifier quoting. Order matters — each stage assumes prior stages have completed. The `_COMMANDS` table is sorted longest-first at startup to prevent partial matches.

pulldown-cmark 0.13's table model differs from markdown-it-py: header cells are direct children of `TableHead`, no `TableRow` wrapper. The converter handles this at `End(TableHead)` rather than `End(TableRow)`.

## Pipeline C — SVG to PNG

```
<input>.svg
  → resvg renderer
  → <output>.png
```

A thin wrapper around `resvg`. No layout, no classification, no markdown. Used when the input extension is `.svg`.

## Data types

The canonical intermediate representation is `src/document/types.rs::Document`:

```rust
pub struct Document {
    pub source_path: PathBuf,
    pub pages: Vec<Page>,
    pub metadata: DocumentMetadata,
}

pub struct Page {
    pub page_num: usize,      // 0-indexed
    pub width: f32,
    pub height: f32,
    pub blocks: Vec<Block>,   // sorted by reading_order
}

pub struct Block {
    pub id: usize,
    pub bbox: Bbox,           // (x0, y0, x1, y1); top-left origin, y-down
    pub text: String,
    pub kind: BlockKind,
    pub font_size: f32,
    pub font_name: String,    // "unknown" for PDF path
    pub page_num: usize,
    pub reading_order: usize,
}

pub enum BlockKind {
    Heading { level: u8 },
    Paragraph,
    ListItem { ordered: bool, depth: u8 },
    TableCell { row: usize, col: usize },
    Caption,
    CodeBlock,
    PageNumber,
    RunningHeader,
    RunningFooter,
    Image { path: Option<String> },
    Formula { latex: String, display: bool },       // produced by hybrid path
    Figure { path: Option<String>, caption: Option<String> },  // produced by hybrid path
}
```

Input-specific intermediates — `RawPage` (pre-classification) and `RawTextBlock` — live alongside. `Section`, `ExtractedImage`, and `RenderedDocument` are post-rendering types consumed by format writers.

## Coordinate convention, explicitly

Every bounding box in `cnv`'s internal types (`Bbox`, `RawTextBlock.bbox`, `Block.bbox`, `ImageRef.bbox`) uses:

```
(0, 0)  ────────────────>  x
 │
 │
 ▼
 y
```

Origin top-left. `y0 < y1` means the block extends downward on the page. `bbox.height() = y1 - y0 > 0`.

PDF-native coordinates are `y0 > y1` with origin bottom-left. mupdf transparently converts when we pull values via `block.bounds()` and `page.bounds()`. `pdfium-render`, when added in Phase 3, will need a shim — PDFium returns native coordinates.

Any future integration with the OpenDataLoader Java tools (e.g. reading their JSON output) will need to flip Y.

## Thread-safety rules

- **mupdf is not thread-safe.** The outer loop over input files in `main.rs::main()` is sequential `.iter().map()`. Do not add `par_iter()` to the PDF pipeline.
- `rayon` is in the dependency graph but must not be introduced to PDF processing.
- Other extractors (DOCX, EPUB, PPTX, HTML) are Rust-pure and parallelisable; they are not currently parallelised but could be.

## Known gotchas (running list)

These are the land-mines to remember when modifying the pipeline. Most are also called out in `CLAUDE.md`.

| Gotcha | Impact |
|---|---|
| `font_name` is always `"unknown"` for PDFs | Classifier heading detection is size-only; can't distinguish bold headings from body |
| Glyphs with no ToUnicode mapping are silently dropped | Math-heavy PDFs lose symbols; biggest quality risk |
| `TextBlockType::Vector` blocks are explicitly dropped | Vector-drawn diagrams and some TeX math symbols are lost |
| `RunningFooter` zone (bottom 7%) is stripped by the renderer | Footnote bodies in this zone vanish |
| Ordered list counter doesn't reset across discontiguous runs, and the source `1.` prefix is not stripped | Occasional `1. 1. Introduction` |
| XY-Cut++ splits by block centre, not edges | Full-width spanning blocks get assigned to one column in 2-col layouts unless pre-masked |
| Code-block regex matches any paragraph starting with `return ` | Prose occasionally fenced |
| Page-number regex matches any standalone small integer | A bare number in body text can be mis-stripped |
| Table x-clustering uses `bbox.x0` | Right-aligned numeric columns cluster incorrectly |
| Hybrid mode (`--hybrid docling`) skips local path entirely | No image extraction in hybrid output (Phase 2a limit) |

## Extension points

### Adding a new input type

1. Create `src/newformat/` with an `extractor.rs`.
2. Implement `pub fn extract(path: &Path) -> VtvResult<Document>` — produce a `Document` with classified `Block`s.
3. Add `mod newformat;` to `src/main.rs`.
4. Add variant to `InputType` in `src/cli.rs`, update `from_path`, `default_format`, `supports_format`, `extensions`.
5. Append extension to `SUPPORTED_EXTENSIONS` in `src/cli.rs`.
6. Add arm in `process_one` match.
7. Implement `process_newformat` — typically just call your extractor then `write_document`.

### Adding a new output format

1. Create `src/formats/newformat/mod.rs` exposing a `write(rendered, doc, output_dir, stem) -> VtvResult<()>`.
2. Add `pub mod newformat;` to `src/formats/mod.rs`.
3. Add variant to `Format` enum in `src/cli.rs`.
4. Update `InputType::supports_format` for which input types can use it.
5. Add arm in `write_rendered` in `src/main.rs`.

### Adding a new `BlockKind` variant

1. Add variant to `BlockKind` in `src/document/types.rs`.
2. Add render arm in `src/render/markdown.rs::render_page` (required — match is exhaustive).
3. Add serialisation arm in every format writer that JSON-encodes blocks (`src/formats/json/mod.rs`, `src/formats/kg/mod.rs`).
4. Update `docs/architecture.md` BlockKind table above.
5. Optionally teach the classifier to produce it.

### Adding a new layout-analysis signal

Most naturally lives in `src/layout/classifier.rs`. If the signal is genuinely about layout (reading order) rather than classification, it belongs in `src/layout/xycut.rs` — but be aware that module is a direct port of a Java reference implementation, so deviations should be justified.

## Testing strategy

- **Unit tests** live next to the code they test (`#[cfg(test)] mod tests`). Each module has tests for its pure functions — `src/layout/xycut.rs` has 16, `src/pdf/extractor.rs` has 20+, etc. Run with `cargo test --bin cnv`.
- **Integration tests** live in `tests/`. Two files currently:
  - `tests/golden.rs` — runs the built binary over the PDF corpus. Fast smoke by default; full sweep + snapshot diff behind `#[ignore]`. See file header for invocation.
  - `tests/hybrid.rs` — tests the Docling hybrid path with an in-process `httpmock` server, plus a regression guard that `--hybrid off` still matches the Phase 1 snapshot.
- **Golden corpus** lives at `papers/` and `papers/golden/`. `papers/golden/` carries fixtures copied from OpenDataLoader's test suite (lorem, arXiv samples, Chinese scan, Italian invoice, PDFUA-1 reference suite). `papers/` carries eight ML-paper arXiv PDFs (attention, resnet, clip, gpt3, bert, math, physics, survey).
- **Snapshots** at `tests/snapshots/`. Currently one: `attention_page_1.md`. Regenerate with `GOLDEN_UPDATE=1 cargo test --test golden -- --ignored`.

Run everything with:

```bash
cargo test                                    # fast: unit + smoke
cargo test -- --ignored                       # + full corpus + snapshot
cargo clippy --all-targets -- -D warnings     # style & bug lints
```

## Build notes

- First build is slow (~5–8 min) because `mupdf` bundles ~55 MB of C source. `reqwest` adds a couple of minutes the first time. Subsequent builds are incremental and fast.
- `clang` + `libclang-dev` required for mupdf's bindgen step.
- `reqwest` uses the `rustls-tls` feature (pure-Rust TLS), so no system openssl is needed.
- Release build (`cargo build --release`) produces `target/release/cnv`, a single static-ish binary. LTO is on.
