<p align="center">
  <img src="docs/assets/pdfp-logo.svg" alt="pdf-processor logo" width="220">
</p>

# pdf-processor

`pdf-processor` is a local PDF processor. Its most mature workflow is converting PDFs into AI-friendly markdown, and the same binary now also has inspection, metadata, search, page editing, imposition, and resizing commands.

**What makes pdfp different:**
- **Single binary, zero Python** — no GPU, no venv, just one ~46MB compiled binary
- **Offline-first** — all processing is local; no cloud API calls unless you opt in
- **Full pipeline** — extraction → layout analysis (XY-Cut++) → block classification → Markdown rendering, not just text dumping
- **Built-in eval** — precision/recall metrics for formula detection, heading accuracy, table recall
- **Page operations** — extract, delete, split, reorder, merge, resize, text overlays, impose — all in the same tool
- **Conservative mode** — `--conservative` disables speculative reconstruction for engineering/legal documents

See [docs/architecture.md](docs/architecture.md) for the full architecture and [docs/TOOL_COMPARISON.md](docs/TOOL_COMPARISON.md) for how pdfp compares to Docling, MinerU, Marker, and other tools.

The active codepath is:

1. **Open PDF with MuPDF** — the C-based rendering engine handles all PDF parsing.
2. **Extract text blocks, word positions, and images** — producing positioned `RawWord`s with bounding boxes, baselines, font sizes, and block/line IDs.
3. **Reconstruct reading order with XY-Cut++** — a recursive gap-based layout analysis that separates columns and orders blocks in natural reading flow.
4. **Classify blocks** — into headings, paragraphs, lists, table cells, captions, code, formulas, figures, and page furniture (headers/footers).
5. **Detect tables and formulas** — coordinate-based table detection from word alignment and rule-line geometry; formula detection from centering and math-character heuristics.
6. **Merge** — table/formula/figure blocks are interleaved into the classified text stream with overlap suppression (high-confidence tables block overlapping formula candidates).
7. **Write markdown** — all blocks are serialised to Markdown with the chosen style (`clean`, `faithful`, or `review`).

Each major module has detailed documentation in [`docs/reference/`](docs/reference/README.md).

## Scope

**In active scope:**
- PDF input → Markdown conversion (the core workflow)
- PDF page operations: extract, delete, split, reorder, merge, resize, text overlays
- Imposition: 2-up, booklet
- Inspection: metadata, page geometry, scan detection, text density
- Search: embedded text search with page reporting
- Document info metadata: read, set, clear
- Optional OCR preprocessing via OCRmyPDF
- Optional hybrid Docling assist for hard pages
- Quality evaluation against fixture JSON files

**Remaining polish areas:**
- Formula and standards-quality closeout
- Evaluation and benchmark polish
- PDF operations polish: outlines, annotations, encryption, optimize
- Forms, accessibility, international text, and hybrid routing polish

**Out of active scope at the repo root:**
- DOCX / EPUB / PPTX / HTML conversion (removed in v0.4.0)
- Markdown to Typst (removed in v0.4.0)
- SVG to PNG (removed in v0.4.0)
- RAG / KG / wiki / JSON export modes (removed in v0.4.0)

## Install

Install the latest Linux release with one command:

```sh
curl -fsSL https://github.com/EdwardAstill/pdf-processor/releases/latest/download/install.sh | sh
```

The installer places `pdfp` under `~/.local/share/pdfp`, symlinks it into `~/.local/bin`, and installs OCR dependencies with the platform package manager when they are missing. On Arch-like systems, it uses `yay` or `paru` for OCRmyPDF because OCRmyPDF is distributed through AUR. Set `PDFP_INSTALL_OCR=0` to skip OCR dependency installation.

To build from source, install `clang`/`libclang` and run:

```sh
cargo build --release --bin pdfp
```

To update an existing installation to the latest release:

```sh
pdfp update
```

Use `--check` to see if a newer version is available without downloading:

```sh
pdfp update --check
```

Use `--force` to reinstall the same version (e.g., if files were corrupted):

```sh
pdfp update --force
```

## Usage

`pdf-processor` is a CLI tool. The installed command is `pdfp`.

```sh
pdfp <INPUT> [OPTIONS]
pdfp convert <INPUT> [OPTIONS]
pdfp ocr <INPUT> -o <OUTPUT> [--mode auto|force]
pdfp doctor [--json]
pdfp inspect <INPUT> [--json] [--ocr auto|force]
pdfp metadata <show|set|clear> ...
pdfp search <INPUT> <TEXT> [--json] [--ocr auto|force]
pdfp eval <FIXTURES_DIR>
pdfp pages <extract|delete|split|reorder|merge|rotate> ...
pdfp impose <2up|booklet> ...
pdfp page <resize|crop|text> <INPUT> -o <OUTPUT>
pdfp update [--check] [--force]
```

Bare `pdfp <INPUT>` is an alias for `pdfp convert <INPUT>`. Every command prints help with `--help`.

New? Start with the [quickstart](docs/quickstart.md) for the three most common workflows.
Full CLI reference and all examples in [`docs/CLI.md`](docs/CLI.md). How pdfp compares to other tools: [`docs/TOOL_COMPARISON.md`](docs/TOOL_COMPARISON.md).

### Convert

Key flags:

| Flag | Default | Description |
| --- | --- | --- |
| `-o`, `--output <DIR>` | next to input | Output directory |
| `--images` | off | Also save detected figures/images under `images/` |
| `--tables` | off | Also save detected table crops under `tables/` |
| `--equations` | off | Also save detected equation crops under `equations/` |
| `--pages <RANGE>` | all | Convert selected pages, e.g. `1-3,9` |
| `--ocr <MODE>` | `auto` | OCR preprocessing: `auto`, `force`, or `off` |
| `--lang <LANGS>` | `eng` | OCR languages, such as `eng+deu` |
| `-v`, `--verbose` | off | Print progress to stderr |

Common patterns:

```sh
# Default Markdown conversion: writes paper.md
pdfp paper.pdf

# Markdown plus optional visual assets
pdfp paper.pdf --images --tables --equations

# Whole directory
pdfp convert papers/ -o out/ --verbose

# Force OCR when the embedded text layer is damaged
pdfp bad-text-layer.pdf --ocr force --lang eng
```

Local OCR uses `ocrmypdf` and Tesseract. Conversion defaults to `--ocr auto`, which skips OCR for clean born-digital PDFs and uses OCR when scan triage says it is needed. Check availability with `pdfp doctor`.

### OCR PDF

Use `pdfp ocr` when you want a searchable PDF as its own artifact:

```sh
# Create a searchable PDF, skipping readable pages
pdfp ocr scan.pdf -o scan.searchable.pdf --mode auto --lang eng

# Force a fresh OCR layer when the existing text layer is damaged
pdfp ocr bad-text-layer.pdf -o fixed.searchable.pdf --mode force --lang eng

# Print machine-readable OCR provenance
pdfp ocr scan.pdf -o scan.searchable.pdf --json
```

`--mode auto` is the safer default. It copies readable PDFs to the requested output without requiring OCRmyPDF. `--mode force` rasterizes all pages through OCRmyPDF, so it is slower and can flatten interactive PDF features.

### Inspect and Search

```sh
# Human page/scan summary
pdfp inspect paper.pdf

# Machine-readable page metadata and scan-like signals
pdfp inspect paper.pdf --json

# Include the OCR decision/provenance in JSON
pdfp inspect scan.pdf --ocr auto --json

# Find embedded text and report matching pages
pdfp search paper.pdf "Fourier"
pdfp search paper.pdf "Fourier" --json

# Search a scan after local OCR preprocessing
pdfp search scan.pdf "invoice" --ocr auto --json
```

By default, `search` uses text already present in the PDF. Add `--ocr auto` or `--ocr force` when image-only scans or damaged text layers need a searchable OCR derivative first.

### Metadata

Use `pdfp metadata` for document information dictionary fields such as title, author, subject, keywords, creator, producer, creation date, and modification date.

```sh
# Show document information metadata
pdfp metadata show paper.pdf
pdfp metadata show paper.pdf --json

# Write a new PDF with updated fields
pdfp metadata set paper.pdf -o paper.metadata.pdf \
  --title "Revised Report" \
  --author "Engineering Team" \
  --keywords "pdf,metadata"

# Preserve the existing modification date while changing text fields
pdfp metadata set paper.pdf -o paper.titled.pdf \
  --title "Revised Report" \
  --no-touch-mod-date

# Set dates with `now`, RFC3339, or raw PDF date syntax
pdfp metadata set paper.pdf -o paper.dated.pdf \
  --creation-date 2026-05-19T12:30:00Z \
  --mod-date now

# Clear selected fields
pdfp metadata clear paper.pdf -o paper.cleaned.pdf --fields title,author
```

Metadata writes are Info-dictionary only. If a PDF also has XMP metadata, `pdfp` preserves it and reports a warning because XMP can still contain older values. PDFs that appear to contain signature fields are refused by default; use `--force-signed` only when you accept that writing a new file may invalidate signatures. Encrypted/password-protected PDFs are refused because rewriting them could remove encryption or produce an incomplete file; decrypt a copy first, for example with `qpdf --decrypt input.pdf decrypted.pdf`.

### Evaluation

Use `pdfp eval` to run the local conversion pipeline against fixture JSON files
and report emitted formula recall, formula detection recall from `debug/formulas/index.json`,
LaTeX snippet recall, heading accuracy, and table recall:

```sh
pdfp eval tests/eval_fixtures/
```

Fixtures are documented in [docs/TESTING.md](docs/TESTING.md#evaluation-pdfp-eval).
Missing local corpus PDFs are reported as skipped instead of crashing the run.

### Page Operations

```sh
# Extract pages 1-3 and 9 into a new PDF
pdfp pages extract input.pdf --pages 1-3,9 -o excerpt.pdf

# Delete selected pages and write a new PDF
pdfp pages delete input.pdf --pages 2,4-6 -o edited.pdf

# Split into chunks of 10 pages
pdfp pages split input.pdf --every 10 -o chunks/

# Reorder selected pages into a new PDF
pdfp pages reorder input.pdf --pages 1,3,2,4-8 -o reordered.pdf

# Merge PDFs in order
pdfp pages merge one.pdf two.pdf three.pdf -o merged.pdf

# Set page rotation on selected pages
pdfp pages rotate input.pdf --pages 1,3 --degrees 90 -o rotated.pdf
```

Page selections are 1-indexed and support `1`, `1-3`, comma lists, `odd`, `even`, and `all`. These commands refuse to overwrite the input path; there is no in-place editing mode yet.

### Imposition and Page Editing

```sh
# Two source pages per output page
pdfp impose 2up input.pdf -o two-up.pdf

# Booklet spread order, padded with blanks as needed
pdfp impose booklet input.pdf -o booklet.pdf

# Resize pages to a target paper size
pdfp page resize input.pdf --paper a4 --fit contain -o resized.pdf

# Set CropBox on selected pages (x0 y0 x1 y1, PDF points)
pdfp page crop input.pdf --pages all --box 0 0 500 700 -o cropped.pdf

# Add searchable red text 36 points from the top-left page edge
pdfp page text input.pdf -o labelled.pdf --pages 1 \
  --text "APPROVED" --x 36 --y 36 --origin top-left \
  --font helvetica-bold --font-size 18 --colour '#cc0000'
```

Text positions are baseline coordinates in PDF points. The default origin is
the bottom-left page edge; use `--origin top-left` for top-down placement.
Built-in Helvetica, Times, and Courier variants are available.

## Tests

```sh
cargo test
```

See [`docs/TESTING.md`](docs/TESTING.md) for the full matrix.

## Documentation

| Document | Covers |
|---|---|
| [docs/quickstart.md](docs/quickstart.md) | First 5 minutes — three most common workflows |
| [docs/CLI.md](docs/CLI.md) | Full CLI reference with all options and examples |
| [docs/architecture.md](docs/architecture.md) | Module map and design decisions |
| [docs/TESTING.md](docs/TESTING.md) | Test matrix, eval fixtures, quality improvement loop |
| [docs/TOOL_COMPARISON.md](docs/TOOL_COMPARISON.md) | How pdfp compares to other tools |
| [docs/reference/](docs/reference/README.md) | Module-by-module code reference |
| [wiki/](wiki/README.md) | Deep implementation notes per pipeline stage |
