# pdfp Architecture

## pdfp's competitive moat

pdfp holds a unique position among PDF tools. These are the things that make it different — and that must be protected in every change:

1. **Single binary, zero Python** — No Python runtime, no GPU required, no venv. One ~46MB compiled binary does everything. This is the strongest differentiator against Docling, MinerU, and every other PDF→Markdown tool.

2. **Offline-first, privacy-respecting** — All processing happens locally. No cloud API calls unless explicitly opted into (`--hybrid docling`). Works in air-gapped environments, compliance-sensitive workflows, and CI pipelines.

3. **Full extraction→layout→classify→render pipeline** — Not just text dumping. pdfp reconstructs reading order (XY-Cut++), classifies blocks (headings, paragraphs, lists, tables, captions, code, formulas), detects tables and formulas, and renders structured Markdown. Most Rust PDF tools stop at text extraction.

4. **Custom eval framework** — No other Rust PDF tool has precision/recall metrics for formula detection, heading accuracy, table recall, figure retention, and decorative image suppression. This is essential for measuring quality over time.

5. **Conservative/review-safe mode** — `--conservative` disables speculative reconstruction (tables as Markdown tables, formulas as LaTeX) and emits only layout-preserving output. Critical for engineering/legal/standards documents where accuracy > prettiness.

6. **Page operations in the same binary** — Extract, delete, split, reorder, merge, resize, searchable text overlays, impose (2-up, booklet). No need for separate tools like qpdf or pdftk.

7. **Built-in eval fixtures** — Labelled regression fixtures for headings, formulas, tables, figures, and decorative images. Run `pdfp eval tests/eval_fixtures/` to measure quality changes over time.

## Module map

```
src/
├── main.rs             # Entry point, dispatches to commands
├── lib.rs              # Library root
├── cli.rs              # Clap CLI definition (commands, args, options)
├── commands.rs         # Command dispatch (match AppCommand → processor)
├── batch.rs            # Input resolution (file/dir/glob → Vec<PathBuf>)
├── error.rs            # PdfpError enum + PdfpResult<T> type alias
│
├── document/           # Shared types
│   ├── mod.rs
│   └── types.rs        # Document, Page, Block, BlockKind, Bbox
│
├── pdf/                # PDF extraction (MuPDF integration)
│   ├── mod.rs
│   ├── extractor.rs    # PdfExtractor: open, extract text/images/metadata
│   ├── metadata.rs     # Font metadata (pdfium feature, optional)
│   └── text_cleanup.rs # Text normalisation
│
├── layout/             # Layout analysis
│   ├── mod.rs
│   ├── xycut.rs        # XY-Cut++ reading order algorithm
│   ├── classifier.rs   # Block classification (heading, paragraph, list...)
│   ├── table.rs        # Table detection (numeric, alignment, form-column cluster)
│   ├── table_detector.rs # Geometry-ruling table candidate detection
│   ├── table_inference.rs # Implicit numeric table / form field inference
│   ├── furniture.rs    # Page furniture (headers/footers/page numbers)
│   └── drawing_ops.rs  # Drawing operation analysis
│
├── formula/            # Formula detection
│   ├── mod.rs
│   ├── detect.rs       # Formula candidate detection (word-geometry heuristics)
│   ├── geometric.rs    # Geometric LaTeX recovery from word positions
│   ├── visual.rs       # Visual formula band scanning
│   ├── ocr.rs          # Formula OCR dispatch and subprocess sidecar
│   └── ocr_onnx.rs     # ONNX Runtime formula OCR (optional feature)
│
├── figure/             # Figure/image extraction
│   ├── mod.rs
│   ├── detect.rs       # Figure region detection
│   └── render.rs       # Figure snapshot rendering
│
├── pipeline/           # PDF → Document pipeline glue
│   ├── mod.rs          # Main pipeline: extract → classify → detect → merge → write
│   └── merge.rs        # Block suppression and media merge helpers
│
├── render/             # Markdown output
│   ├── mod.rs
│   ├── markdown/       # MarkdownRenderer (mod.rs), clean-style reflow (clean.rs), tests
│   ├── table.rs        # Markdown table, numeric-table, and form-field rendering
│   ├── scholarly.rs    # Scholarly front-matter rendering
│   ├── media.rs        # Image/figure dedup plan
│   └── text.rs         # Table cell escaping
│
├── formats/            # Output format writers
│   ├── mod.rs
│   └── raw/mod.rs      # Single .md output
│
├── hybrid/             # Docling hybrid backend
│   ├── mod.rs          # Orchestration, routing policy
│   ├── client.rs       # HTTP client (reqwest blocking)
│   ├── triage.rs       # Per-page routing decisions
│   └── page_extract.rs # Single-page PDF extraction
│
├── ocr/                # OCR preprocessing
│   ├── mod.rs          # OCRmyPDF integration, decision logic
│   └── triage.rs       # Scan detection
│
├── processor/          # CLI command implementations
│   ├── mod.rs
│   ├── inspect.rs      # pdfp inspect
│   ├── search.rs       # pdfp search
│   ├── metadata.rs     # pdfp metadata (read/write/clear)
│   ├── ocr_cmd.rs      # pdfp ocr
│   ├── pages.rs        # pdfp pages (extract/delete/split/reorder/merge)
│   ├── page_range.rs   # Page range parsing
│   ├── impose.rs       # pdfp impose (2up/booklet)
│   ├── resize.rs       # pdfp page resize/crop dispatch
│   ├── text.rs         # pdfp page text searchable overlays
│   ├── doctor.rs       # pdfp doctor
│   └── update.rs       # pdfp update (check/force)
│
└── eval/               # Quality evaluation framework
    ├── mod.rs
    ├── fixtures.rs     # JSON fixture loading
    ├── metrics.rs      # Precision/recall computation
    └── runner.rs       # Eval runner
```

## Key design decisions

### MuPDF as the rendering engine

pdfp uses MuPDF (AGPL) via the `mupdf` 0.6 crate. MuPDF was chosen because:
- It's the fastest PDF renderer
- It bundles as C source, compiled via `cc` — no system library dependency
- It handles text extraction, image extraction, and page rendering
- The AGPL is compatible with pdfp's MIT license as long as pdfp remains open-source

Tradeoff: MuPDF is not thread-safe. Outer loops are sequential `.iter().map()`. Do not use `rayon`/`par_iter` for PDF processing.

MuPDF 0.6 does not expose font family names through its Rust wrapper. The `pdfium-metadata` optional feature adds a second-opinion reader for font names and struct-tree access.

### XY-Cut++ layout analysis

The reading order algorithm is a Rust port of OpenDataLoader's `XYCutPlusPlusSorter.java` (Apache-2.0), based on arXiv:2504.10258. It uses recursive gap-based segmentation with narrow-outlier retry on the X axis. The coordinate system is top-left origin (Y grows downward), which is the opposite of PDF's Y-up — every Y comparison in the algorithm is flipped relative to the Java reference.

### lopdf for low-level PDF manipulation

`lopdf` 0.40 is used for metadata read/write and PDF object manipulation. It operates at the raw PDF object level (dictionaries, arrays, streams, references). pdfp uses it for document information dictionary fields (title, author, subject, keywords, creator, producer, dates), page boxes, and searchable text-overlay content streams. The same approach works for reading annotation dictionaries and form field values.

### mupdf's pdf module (currently unused)

mupdf 0.6 exposes a rich `pdf` sub-module (`PdfDocument`, `PdfPage`, `PdfAnnotation`, `PdfWriteOptions`) that pdfp does not currently use. This module provides:
- Outline/bookmark extraction
- Annotation enumeration and creation
- Page rotation, crop box, media box
- PDF save with compression, linearization, encryption, garbage collection

Page rotation and crop-box editing are implemented; outlines, annotations, compression, and encryption remain future PDF-operations polish.

### Coordinate system

MuPDF uses `Rect { x0, y0, x1, y1 }` — corner coordinates, not `(x, y, w, h)`. `Bbox` mirrors this. Origin is top-left, Y increases downward. This is consistent through the entire pipeline.

### Error types

`PdfpError` (formerly `VtvError`) is the library error type using `thiserror`. `PdfpResult<T>` is the convenience alias. CLI commands use `anyhow::Result` at the outermost level and convert from `PdfpError` where needed.

### `<!-- page:N -->` markers

The `MarkdownRenderer` emits page markers (1-indexed) consumed by `split_into_sections()` for page tracking. These are stripped from section content in the final output.

### Heading level derivation

Heading levels are derived from font size ratio against the body text mode:
- >=2.0× body → H1
- >=1.6× body → H2
- >=1.35× body → H3
- >=1.15× body → H4
- else → H5

With the `pdfium-metadata` feature, struct-tree roles (`H1`..`H6`, `Title`) override this, and bold-at-body-size gets promoted to H4.

## Test structure

Full test structure is documented in [`docs/TESTING.md`](docs/TESTING.md). Quality evaluation fixtures live in `tests/eval_fixtures/`.

- `cargo clippy --all-targets -- -D warnings` — must be clean before committing
- `cargo test` — runs unit tests + integration tests
- `pdfp eval tests/eval_fixtures/` — quality evaluation against fixtures

## Remaining work

- Formula standards-quality closeout: better table/formula separation, LaTeX recovery quality
- Evaluation and benchmark polish: expand labelled fixtures, optional sidecar tools
- PDF operations: outlines, annotations, encryption, compression via existing MuPDF/lopdf APIs
- Forms, accessibility metadata, international text, and hybrid routing polish

## References

- `docs/reference/README.md` — module-by-module code reference with key types, functions, and CLI flags
- `docs/CLI.md` — full CLI reference
- `docs/TESTING.md` — test matrix
- `docs/TOOL_COMPARISON.md` — comparison against competitors
- `docs/reference/pdf-format.md` — PDF file format primer
