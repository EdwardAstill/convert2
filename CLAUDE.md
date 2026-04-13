# cnv — convert2 PDF-to-Markdown CLI

## Project purpose

`cnv` converts PDF files into AI-friendly markdown structures. It solves the problem of feeding PDF content into LLM pipelines by extracting text with layout analysis, classifying blocks by semantic role, and writing output in one of four formats optimized for different downstream uses (plain markdown, RAG chunking, Obsidian-style wikilinks, or a knowledge graph).

## Architecture overview

Pipeline per PDF:

```
PDF file
  └─ PdfExtractor::extract()        → (Vec<RawPage>, DocumentMetadata)
       └─ build_xycut_tree()         → XyCutNode  (reading order tree)
       └─ assign_reading_order()     → mutates RawTextBlock.reading_order
       └─ Classifier::classify_page() → Vec<Block>  (with BlockKind assigned)
           └─ assembled into Document { pages, metadata, source_path }
               └─ MarkdownRenderer::render_document() → RenderedDocument
                   └─ Format::write()  → files on disk
```

### Module map

- `src/pdf/extractor.rs` — mupdf integration; `PdfExtractor::extract()` opens the PDF once and returns all `RawPage`s plus `DocumentMetadata`.
- `src/layout/xycut.rs` — XY-Cut++ algorithm; `build_xycut_tree()` returns an `XyCutNode` tree, `assign_reading_order()` walks it to stamp `reading_order` onto each `RawTextBlock`.
- `src/layout/classifier.rs` — font-size-based block classification; `Classifier::new_for_document()` computes the body font size as the document-wide mode, then `classify_page()` assigns `BlockKind` to each block.
- `src/document/types.rs` — all shared structs: `Bbox`, `RawTextBlock`, `ImageRef`, `Block`, `BlockKind`, `RawPage`, `Page`, `DocumentMetadata`, `Document`, `Section`, `ExtractedImage`.
- `src/render/markdown.rs` — `MarkdownRenderer` turns `Document` → `RenderedDocument` (full markdown string + `Vec<Section>` + extracted images); emits `<!-- page:N -->` markers and calls `split_into_sections()` which parses them to populate `Section.page_start`/`page_end`.
- `src/formats/raw.rs` — writes `<stem>.md` + `images/` directory.
- `src/formats/rag.rs` — splits into `<stem>_chunk_NNNN.md` files (~500 tokens each, configurable) with YAML frontmatter (source, chunk_index, total_chunks, section_title, page_start, page_end).
- `src/formats/karpathy.rs` — one `.md` per section (slugified filename) with `[[WikiLink]]` cross-refs injected, plus `index.md` listing all sections.
- `src/formats/kg.rs` — writes `<stem>_graph.json` containing Section/Concept/Citation nodes and Contains/Cites/RelatedTo weighted edges.
- `src/batch.rs` — `resolve_inputs()` accepts a file path, directory, or glob and returns `Vec<PathBuf>`; `output_dir_for()` computes the output directory.
- `src/cli.rs` — clap `Cli` struct and `Format` enum (`Raw`, `Rag`, `Karpathy`, `Kg`).
- `src/main.rs` — orchestrates the pipeline in a sequential `.iter().map()` loop (no rayon — see gotchas).
- `src/error.rs` — `VtvError` (thiserror) and `VtvResult<T>` alias.

## Key design decisions and gotchas

- **mupdf 0.6.0 does not expose font names.** `TextChar` only has `char()`, `origin()`, `size()`, `quad()`. `PdfExtractor::dominant_font_name()` always returns `"unknown"`. The classifier uses font size only for heading detection — do not add font-name logic without first verifying the mupdf Rust wrapper version exposes it.

- **mupdf is not thread-safe.** `par_iter` / rayon was removed. The outer loop in `main.rs` is a plain sequential `.iter().map()`. Do not re-introduce `par_iter` without confirming mupdf thread safety.

- **mupdf `Rect` is `(x0, y0, x1, y1)` corner coords, not `(x, y, w, h)`.** `Bbox` mirrors this exactly. Page width/height are computed as `bounds.x1 - bounds.x0` / `bounds.y1 - bounds.y0`.

- **Coordinate origin is top-left, Y increases downward.** This matches screen/PDF convention. `Bbox::vertical_gap_to(other)` returns `other.y0 - self.y1` (positive = gap below).

- **`TextPageFlags::PRESERVE_IMAGES` must be passed** to `page.to_text_page()` to get `TextBlockType::Image` blocks; omitting it silently drops all image refs.

- **`<!-- page:N -->` markers** are emitted by `MarkdownRenderer::render_page()` (1-indexed) and consumed by `split_into_sections()` to track page boundaries on `Section`. Markers are stripped from section content.

- **Table cell detection uses `block_id` as the HashMap key** (stable index within the page, assigned by mupdf block enumeration), not positional index into the vec, which can shift after sorting.

- **Heading level is derived from font size ratio** against the document-mode body size. Ratios: >=2.0→H1, >=1.6→H2, >=1.35→H3, >=1.15→H4, else H5. `heading_size_ratio` config (default 1.15) is the minimum ratio to trigger any heading classification.

- **KG concept nodes require frequency >= 2** to filter single-occurrence noise. Concepts that match a section title are suppressed (they already have a Section node).

- **RAG overlap** is approximated by char count (`overlap_tokens * 4`), not true token count. The `estimate_tokens` function uses `len / 4` throughout.

- **First `cargo build` is slow** (~5 min). mupdf bundles ~55 MB of C source compiled via `cc` during the build. Requires `clang` and `bindgen` dependencies. Subsequent builds are incremental.

- **`rayon` is listed in `Cargo.toml`** but is not currently used. It was removed from the hot path to fix a thread-safety crash. The dependency is retained but must not be used for PDF processing.

## Build and test

```bash
# Prerequisites: clang, libclang-dev (for mupdf bindgen)
cargo build --release        # binary at target/release/cnv
cargo test                   # 20 unit tests across xycut, classifier, renderer
cargo clippy -- -D warnings  # must be clean before committing
```

### Usage

```bash
cnv paper.pdf                        # raw format, output next to input
cnv paper.pdf -f rag --chunk-size 800
cnv paper.pdf -f karpathy -o ./notes/
cnv paper.pdf -f kg
cnv "papers/*.pdf" -f rag -o ./out/  # glob input
cnv ./papers/ -f raw                 # directory input
```

CLI flags: `--format` (`raw`|`rag`|`karpathy`|`kg`), `--output`, `--chunk-size`, `--min-h-gap`, `--min-v-gap`, `--no-images`, `--verbose`.

## Adding a new output format

1. Create `src/formats/newformat.rs`. Implement a struct with a `write(rendered: &RenderedDocument, doc: &Document, output_dir: &Path, stem: &str) -> VtvResult<()>` method. Use `VtvError::Io` for all `fs::` errors. (`VtvError`/`VtvResult` are defined in `src/error.rs` — the internal type names were not changed from the original codebase.)

2. Add `pub mod newformat;` to `src/formats/mod.rs`.

3. Add a variant to the `Format` enum in `src/cli.rs`:
   ```rust
   #[derive(ValueEnum, Debug, Clone, PartialEq)]
   pub enum Format {
       Raw, Rag, Karpathy, Kg,
       Newformat,  // add here
   }
   ```

4. Add a match arm in `main.rs` inside `process_one()`:
   ```rust
   Format::Newformat => {
       formats::newformat::NewFormat::write(&rendered, &doc, &output_dir, &stem)
           .with_context(|| format!("Failed to write newformat output to {}", output_dir.display()))?;
   }
   ```

The format receives a fully-rendered `RenderedDocument` (`.markdown: String`, `.sections: Vec<Section>`, `.images: Vec<ExtractedImage>`) and the original `Document` (for metadata and source path). The output directory is pre-determined by `batch::output_dir_for()`; formats create it themselves via `fs::create_dir_all`.
