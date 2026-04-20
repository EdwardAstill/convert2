# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project purpose

`cnv` is a multi-format file converter. It converts documents (PDF, DOCX, EPUB, PPTX, HTML) into AI-friendly markdown, converts Markdown to Typst, and converts SVG to PNG. Single self-contained Rust binary with no runtime dependencies.

## Build and test

```bash
# Prerequisites: clang, libclang-dev (for mupdf bindgen)
cargo build --release        # binary at target/release/cnv
cargo test                   # 95 unit tests
cargo clippy -- -D warnings  # must be clean before committing

# Run a single test
cargo test typst::latex2typst::tests::test_frac
# Run all tests in a module
cargo test typst::converter::tests
```

First `cargo build` is slow (~5 min) because mupdf bundles ~55 MB of C source compiled via `cc`. Subsequent builds are incremental.

### Usage

```bash
cnv paper.pdf                          # PDF → raw markdown (default)
cnv paper.pdf -f rag --chunk-size 800  # PDF → RAG chunks
cnv paper.docx -f karpathy             # DOCX → wiki-style markdown
cnv book.epub                          # EPUB → raw markdown
cnv slides.pptx -o ./notes/            # PPTX → markdown
cnv page.html -f kg                    # HTML → knowledge graph JSON
cnv notes.md                           # Markdown → Typst (default for .md)
cnv notes.md --paper a4 --stdout       # Markdown → Typst with paper size, to stdout
cnv diagram.svg                        # SVG → PNG (default for .svg)
cnv "papers/*.pdf" -f rag -o ./out/    # glob input
cnv ./papers/                          # directory input (all supported types)
```

## Architecture overview

Three conversion pipeline families, dispatched by input file extension in `main.rs::process_one()`:

### Pipeline A: Document → Markdown (PDF, DOCX, EPUB, PPTX, HTML)

```
Input file → Extractor → Document → MarkdownRenderer → RenderedDocument → Format writer
                                                                           ├─ raw (single .md)
                                                                           ├─ rag (chunked .md files)
                                                                           ├─ karpathy (wiki folder)
                                                                           └─ kg (JSON graph)
```

Each input type has its own extractor that produces a `Document`. The rest of the pipeline (rendering + format writing) is shared.

**PDF pipeline** is the most complex — it goes through additional stages:
```
PdfExtractor::extract() → (Vec<RawPage>, DocumentMetadata)
  → build_xycut_order() → assign_reading_order()    [layout analysis, XY-Cut++]
  → Classifier::classify_page()                     [block classification]
  → Document
```

Layout analysis is a Rust port of OpenDataLoader's `XYCutPlusPlusSorter.java`
(Hancom, Apache-2.0), based on arXiv:2504.10258. Four stages: (1) pre-mask
cross-layout elements (wide headers/titles/footers), (2) compute density ratio
(reserved for future tiebreaker), (3) recursive gap-based segmentation with
narrow-outlier retry on the X axis, (4) merge cross-layout elements back by Y
into the main stream. Coordinate system is top-left origin (y grows downward);
the Java reference uses PDF y-up, so every Y comparison is flipped — see the
module doc-comment in `src/layout/xycut.rs` for the translation table.

**DOCX/EPUB/PPTX** extractors use `zip` + `quick-xml` to parse OOXML/EPUB ZIP archives directly. **HTML** and **EPUB** (for chapter content) use `scraper` (html5ever). These formats already have semantic structure, so no layout analysis or classification is needed — extractors produce `Document` with classified `Block`s directly.

### Pipeline B: Markdown → Typst

```
.md file → pulldown-cmark parser → TypstRenderer → .typ file
                                      └─ latex_to_typst() for math expressions
```

Ported from the Python `md2typ` project. The LaTeX→Typst math translation is a 5-stage pipeline in `src/typst/latex2typst.rs` (environments → structured commands → simple replacements → scripts → identifier quoting). Config loaded from `$XDG_CONFIG_HOME/mdtyp/config.toml` or `--typst-config`.

### Pipeline C: SVG → PNG

```
.svg file → resvg → .png file
```

### Input/output format validation

Format is auto-detected from extension. `InputType::supports_format()` in `cli.rs` validates combinations:
- `.pdf/.docx/.epub/.pptx/.html` → `raw`, `rag`, `karpathy`, `kg`, `json`
- `.md` → `typst`
- `.svg` → `png`

`-f` defaults to `raw` for documents, `typst` for `.md`, `png` for `.svg`.

### Key types (src/document/types.rs)

`Document { pages, metadata, source_path }` is the universal intermediate representation. All extractors produce it; the renderer and format writers consume it. `Page` contains `Vec<Block>`, each with a `BlockKind` (Heading, Paragraph, ListItem, TableCell, Caption, CodeBlock, PageNumber, RunningHeader, RunningFooter, Image).

`RenderedDocument { markdown, sections, images }` is the markdown-rendered form consumed by format writers.

### Module map

| Module | Purpose |
|---|---|
| `src/main.rs` | Pipeline dispatch by `InputType`, shared `write_document()` |
| `src/cli.rs` | Clap CLI, `Format` enum, `InputType` enum with validation |
| `src/batch.rs` | `resolve_inputs()` (file/dir/glob → `Vec<PathBuf>`), `output_dir_for()` |
| `src/document/types.rs` | All shared types: `Document`, `Page`, `Block`, `BlockKind`, `Bbox`, etc. |
| `src/error.rs` | `VtvError` (thiserror) and `VtvResult<T>` |
| `src/pdf/extractor.rs` | mupdf integration, text/image extraction |
| `src/layout/xycut.rs` | XY-Cut++ reading order algorithm |
| `src/layout/classifier.rs` | Font-size-based block classification |
| `src/render/markdown.rs` | `Document` → `RenderedDocument` |
| `src/formats/{raw,rag,karpathy,kg,json}/mod.rs` | Output format writers |
| `src/docx/extractor.rs` | DOCX extraction (zip + quick-xml) |
| `src/epub/extractor.rs` | EPUB extraction (zip + quick-xml + scraper) |
| `src/pptx/extractor.rs` | PPTX extraction (zip + quick-xml) |
| `src/html_extract/extractor.rs` | HTML extraction (scraper) |
| `src/typst/converter.rs` | Markdown → Typst (pulldown-cmark event-driven renderer) |
| `src/typst/latex2typst.rs` | LaTeX math → Typst math translation |
| `src/typst/config.rs` | Typst converter TOML config |
| `src/svg/converter.rs` | SVG → PNG (resvg) |
| `src/hybrid/mod.rs` | Hybrid backend — `apply_to_document` + `RoutingPolicy` + `HybridStats` |
| `src/hybrid/client.rs` | `docling-serve` HTTP client (reqwest blocking) |
| `src/hybrid/triage.rs` | Per-page routing decision (math density, tables, scan density) |
| `src/hybrid/page_extract.rs` | Extract a single PDF page as bytes for upload |
| `src/pdf/metadata.rs` | Phase 3 — optional font-name / weight / struct-tree sidecar (feature `pdfium-metadata`) |

### Hybrid backend (Phase 2b — per-page routing)

`--hybrid docling` runs the local mupdf pipeline first, then triages each
resulting page: math-heavy pages, pages with detected tables, and pages with
very low text density (likely scanned) are extracted as single-page PDFs,
uploaded to `docling-serve`, and the returned markdown replaces the
block-rendered output for just those pages. Simple prose pages stay on the
fast local path and keep their images. Controlled via:

- `--hybrid <off|docling>` (default `off`)
- `--hybrid-url <url>` (default `http://localhost:5001`)
- `--hybrid-timeout-secs <n>` (default `600`)
- `--hybrid-policy <auto|all>` (default `auto`)

`auto` uses the triage heuristics in `src/hybrid/triage.rs`. `all` routes
every page (useful for tests and for users who want uniform Docling output).

Per-page backend failures are logged and non-fatal: the affected page keeps
its locally-rendered output and processing continues. The process exits 0.

Pages that were routed carry their markdown in `Page.override_markdown`, and
the renderer (`src/render/markdown.rs::render_page`) checks that field before
serialising blocks. This is the "block-level merge" in Phase 2b — at page
granularity, since docling-serve returns markdown per whole-PDF request.

### Phase 3 — font-name metadata (optional, feature `pdfium-metadata`)

mupdf 0.6's Rust wrapper does not surface font family, weight, or italic
flags, so the classifier is stuck on font-size alone. Building with
`--features pdfium-metadata` pulls in `pdfium-render` as a second-opinion
PDF reader that exposes those attributes, which the classifier then layers
on top of the size-ratio signal:

- A struct-tree role (`H1`..`H6`, `Title`) overrides everything — tagged PDFs
  get authoritative heading detection.
- Bold at body size (weight ≥ 700, short line, not sentence-terminated) is
  promoted to `Heading { level: 4 }` — rescues documents with no size
  hierarchy.
- Default build (feature off) falls back to size-only behaviour, unchanged.

The feature flag dynamically loads `libpdfium` at runtime, so users must
install it separately (apt/pacman/brew or pdfium-binaries releases). When
the library is missing or fails to load, the loader logs a warning and the
classifier silently degrades to size-only — it never panics.

## Key design decisions and gotchas

- **mupdf 0.6.0 does not expose font names.** Classification uses font size only. Do not add font-name logic without verifying the mupdf wrapper version.

- **mupdf is not thread-safe.** The outer loop in `main.rs` is sequential `.iter().map()`. Do not use `par_iter`/rayon for PDF processing.

- **mupdf `Rect` is `(x0, y0, x1, y1)` corner coords, not `(x, y, w, h)`.** `Bbox` mirrors this. Coordinate origin is top-left, Y increases downward.

- **`TextPageFlags::PRESERVE_IMAGES` must be passed** to `page.to_text_page()` or image blocks are silently dropped.

- **`<!-- page:N -->` markers** are emitted by `MarkdownRenderer` (1-indexed) and consumed by `split_into_sections()` for page tracking. Stripped from section content.

- **Heading level is derived from font size ratio** against body mode: >=2.0→H1, >=1.6→H2, >=1.35→H3, >=1.15→H4, else H5.

- **RAG token estimation** uses `len / 4` (char count proxy), not real tokenization.

- **LaTeX→Typst pipeline order matters.** Stages must run in sequence: environments → structured commands → simple replacements → scripts → identifier quoting. Each stage assumes prior stages have completed. The `_COMMANDS` list is sorted longest-first at runtime to prevent partial matches.

- **pulldown-cmark table model differs from markdown-it-py.** Header cells are direct children of `TableHead` (no `TableRow` wrapper). The Typst converter handles this at `End(TableHead)` rather than `End(TableRow)`.

- **Rust `regex` crate does not support backreferences.** The aligned environment regex uses duplicate alternation `(?:aligned|align\*?)` instead of `\1`.

- **quick-xml 0.39 API:** Use `BytesText::decode()` (not `unescape()`) and `Attribute::unescape_value()`.

- **`rayon` is listed in `Cargo.toml`** but must not be used for PDF processing due to mupdf thread-safety.

- **Error types use legacy naming:** `VtvError`/`VtvResult` in `src/error.rs` — inherited from original codebase, not renamed.

## Adding a new input extractor

1. Create `src/newformat/mod.rs` and `src/newformat/extractor.rs`
2. Implement `pub fn extract(path: &Path) -> VtvResult<Document>` — produce a `Document` with classified `Block`s
3. Add `mod newformat;` to `src/main.rs`
4. Add variant to `InputType` in `src/cli.rs`, update `from_path()`, `default_format()`, `supports_format()`, `extensions()`
5. Add extension(s) to `SUPPORTED_EXTENSIONS` in `src/cli.rs`
6. Add `InputType::NewFormat => process_newformat(path, cli, &format)` in `process_one()` match
7. Implement `process_newformat()` — typically just calls your extractor then `write_document()`

## Adding a new output format

1. Create `src/formats/newformat.rs` with `write(rendered: &RenderedDocument, doc: &Document, output_dir: &Path, stem: &str) -> VtvResult<()>`
2. Add `pub mod newformat;` to `src/formats/mod.rs`
3. Add variant to `Format` enum in `src/cli.rs`
4. Update `InputType::supports_format()` for which input types can use it
5. Add match arm in `write_document()` in `main.rs`
