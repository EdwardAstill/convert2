# vtv

`vtv` converts PDF documents into AI-friendly markdown structures. It is a single self-contained binary with no runtime dependencies on Java, Python, or a GPU. All processing happens locally. Given a PDF file (or a directory or glob of files), `vtv` extracts text with bounding boxes via MuPDF, reconstructs reading order using an XY-Cut++ algorithm, classifies blocks into headings, lists, tables, captions, and body text, then writes one of four output formats suited to different downstream uses: raw markdown, RAG-ready chunks, a Karpathy-style wiki folder, or a JSON knowledge graph.

---

## How it works

1. **Extraction.** MuPDF opens the PDF and walks its text blocks, recording each block's bounding box, dominant font size, and text content. Image block positions are recorded for reference.

2. **Reading order.** The XY-Cut++ algorithm partitions the bounding boxes of all text blocks on a page into a binary tree by finding the largest whitespace gaps — first trying a vertical cut (column boundary), then a horizontal cut (paragraph boundary). An in-order traversal of that tree yields the correct reading sequence. This correctly handles two-column academic papers, newspaper layouts, and similar multi-column arrangements where a naive top-to-bottom sort would interleave columns.

3. **Classification.** A document-level classifier computes the statistical mode of all font sizes to establish the body size, then labels each block: `Heading` (levels 1–5 by font-size ratio), `ListItem` (ordered or unordered, with indent depth), `TableCell` (detected via 2D position clustering), `Caption` (matched by prefix pattern), `CodeBlock` (heuristic keyword match), `PageNumber`, `RunningHeader`, `RunningFooter`, and `Paragraph`. Running headers, footers, and page numbers are stripped from all output formats.

4. **Rendering.** Classified blocks are emitted as standard markdown. Tables become GFM pipe tables. Lists respect nesting. Each page boundary is marked with an HTML comment (`<!-- page:N -->`) that is consumed by the section splitter but invisible when the markdown is rendered.

5. **Output format.** The rendered markdown is post-processed into whichever format was requested.

---

## Installation

### Prerequisites

`vtv` bundles MuPDF and compiles it from source during the first `cargo build`. This requires `clang` for the `bindgen`-generated bindings.

| Platform | Command |
| --- | --- |
| Arch Linux | `sudo pacman -S clang` |
| Ubuntu / Debian | `sudo apt install clang` |
| macOS | `xcode-select --install` |

### Build from source

```sh
cargo build --release
cp target/release/vtv ~/.local/bin/   # or anywhere on PATH
```

Or install directly with Cargo:

```sh
cargo install --path .
```

The first build is slow (roughly 5 minutes) because MuPDF is compiled from source. Subsequent builds use the cached artifacts and are fast.

---

## Usage

```
vtv <INPUT> [OPTIONS]
```

`INPUT` can be a single PDF path, a directory (all `.pdf` files inside), or a glob pattern (e.g. `"papers/*.pdf"`).

### Options

| Flag | Default | Description |
| --- | --- | --- |
| `-f`, `--format <FORMAT>` | `raw` | Output format: `raw`, `rag`, `karpathy`, `kg` |
| `-o`, `--output <DIR>` | next to input | Output directory |
| `--chunk-size <N>` | `500` | Target chunk size in approximate tokens (`rag` format only) |
| `--min-h-gap <pts>` | `8.0` | Minimum vertical gap for horizontal cuts (XY-Cut tuning) |
| `--min-v-gap <pts>` | `12.0` | Minimum horizontal gap for vertical cuts (XY-Cut tuning) |
| `--no-images` | off | Skip image extraction |
| `-v`, `--verbose` | off | Print per-file progress to stderr |
| `--version` | | Print version |

### Examples

```sh
# Default: raw markdown next to the input file
vtv paper.pdf

# RAG chunks with a 300-token target
vtv paper.pdf -f rag --chunk-size 300 -o out/

# Karpathy wiki folder
vtv paper.pdf -f karpathy -o wiki/

# Knowledge graph
vtv paper.pdf -f kg -o graph/

# Batch: all PDFs in a directory, verbose
vtv docs/ -f raw -o out/ --verbose

# Glob (quote to prevent shell expansion)
vtv "reports/*.pdf" -f rag -o chunks/
```

---

## Output formats

### `raw` (default)

Writes a single markdown file named `<stem>.md`. If image extraction is enabled, extracted images are written to an `images/` subdirectory alongside the markdown file.

```
out/
  paper.md
  images/
    paper_p1_img0.png
    paper_p2_img0.png
```

The markdown uses standard ATX headings (`#`–`######`), GFM pipe tables, fenced code blocks, and `![image](images/...)` references for any extracted figures.

---

### `rag`

Splits the document into overlapping chunks suitable for embedding and retrieval. Each chunk is a separate markdown file with YAML frontmatter.

```
out/
  paper_chunk_0001.md
  paper_chunk_0002.md
  ...
```

Example frontmatter:

```yaml
---
source: paper.pdf
chunk_index: 1
total_chunks: 14
section_title: "Introduction"
page_start: 1
page_end: 3
---
```

Chunks are split at paragraph boundaries where possible. The overlap between consecutive chunks is approximately 50 tokens (configurable via the source). Token counts use the chars/4 heuristic common for English text.

---

### `karpathy`

One markdown file per section, plus an `index.md`. Cross-references between sections are injected as `[[WikiLinks]]` using a greedy longest-match pass over each file's content. The format is compatible with Obsidian and similar wiki tools.

```
out/
  index.md
  introduction.md
  related_work.md
  methodology.md
  results.md
  conclusion.md
```

`index.md` contains a `[[WikiLink]]` list of all sections:

```markdown
# Paper Title

## Sections

- [[Introduction]]
- [[Related Work]]
- [[Methodology]]
- [[Results]]
- [[Conclusion]]
```

Each section file opens with its heading, then its content with any mentions of other section titles converted to `[[WikiLinks]]`. Self-links are suppressed. Case-insensitive matches that differ in capitalisation from the canonical title use the `[[Canonical Title|matched text]]` alias form.

---

### `kg`

Writes a single `<stem>_graph.json` file containing a knowledge graph with three node types and three edge types.

Node types: `Section`, `Concept`, `Citation`.

Edge types:
- `Contains` — section → concept (weighted by relative concept frequency)
- `Cites` — section → citation
- `RelatedTo` — section → section (when they share at least 2 concepts; weighted by Jaccard-like overlap)

Concepts are multi-word capitalised phrases (2–4 words) that appear in at least two sections. Citations are matched by the pattern `[Author, Year]` or `[N]`. Concepts that duplicate section titles are excluded.

Example structure:

```json
{
  "metadata": {
    "title": "Attention Is All You Need",
    "author": "Vaswani et al.",
    "source": "paper.pdf",
    "section_count": 8,
    "node_count": 42,
    "edge_count": 67
  },
  "nodes": [
    { "id": "sec_introduction", "label": "Introduction", "kind": "Section", "page": 1, "excerpt": "We propose a new simple network architecture..." },
    { "id": "concept_multi_head_attention", "label": "Multi Head Attention", "kind": "Concept", "frequency": 5 },
    { "id": "cite__vaswani_2017_", "label": "[Vaswani, 2017]", "kind": "Citation" }
  ],
  "edges": [
    { "source": "sec_introduction", "target": "concept_multi_head_attention", "relation": "Contains", "weight": 0.8 },
    { "source": "sec_introduction", "target": "cite__vaswani_2017_", "relation": "Cites" },
    { "source": "sec_introduction", "target": "sec_methodology", "relation": "RelatedTo", "weight": 0.4 }
  ]
}
```

---

## XY-Cut++ algorithm

Reading order recovery is non-trivial for PDFs because the file format stores text blocks in drawing order, not reading order. On a two-column page, MuPDF may return blocks interleaved between columns.

`vtv` implements a variant of the XY-Cut algorithm. For a given set of blocks, the algorithm finds the largest horizontal whitespace gap (a gap between rows of blocks) and the largest vertical whitespace gap (a gap between columns). If a vertical gap is more than 20% larger than the best horizontal gap, the page is split vertically first — correctly isolating the left column from the right before any paragraph-level splitting happens. Otherwise the horizontal cut is taken, which preserves natural top-to-bottom reading order within a column. The process recurses on each sub-region until no gap exceeds the configured minimum (8 pt horizontal, 12 pt vertical by default). An in-order traversal of the resulting binary tree gives the reading sequence.

The `--min-h-gap` and `--min-v-gap` flags let you tune the sensitivity. Raising `--min-v-gap` is useful for documents with narrow column gutters; lowering `--min-h-gap` helps when paragraph spacing is tight.

---

## Limitations

- **Scanned PDFs produce no text.** If a PDF contains only rasterised page images (a common result of scanning physical documents), MuPDF returns no text blocks and `vtv` will produce an empty or near-empty output. OCR is not performed.

- **Heading detection uses font size only.** The MuPDF 0.6 Rust wrapper does not expose per-character font names, so bold or small-caps body text at the body size cannot be distinguished from a normal paragraph. All heading detection is based on the font-size ratio relative to the document body size mode.

- **Batch processing is sequential.** MuPDF's C library is not safe to call from multiple threads concurrently. Processing multiple PDFs runs one file at a time rather than in parallel. The `rayon` dependency is present for potential future use within a single document but is not currently applied across files.

- **Table detection is heuristic.** Tables are identified by clustering text block positions into rows and columns. Dense or irregular tables, tables with merged cells, and very large tables (spanning more than 40% of page height, or more than 10 columns) are suppressed to avoid misclassifying two-column body text as a table.
