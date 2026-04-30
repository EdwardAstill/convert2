# cnv

`cnv` is now a PDF-first tool. The primary job is simple: convert PDFs into local, AI-friendly markdown.

The active codepath is:

1. Open PDF with MuPDF.
2. Extract text blocks and images.
3. Reconstruct reading order with XY-Cut++.
4. Classify blocks into headings, lists, tables, captions, and paragraphs.
5. Write markdown plus extracted images.

Legacy experiments and non-core converters were moved under [`legacy/`](legacy/README.md).

## Scope

Current top-level scope:

- PDF input only
- Markdown output only
- Optional hybrid Docling assist for hard pages
- Local-first processing

Out of active scope at the repo root:

- DOCX / EPUB / PPTX / HTML conversion
- Markdown to Typst
- SVG to PNG
- RAG / KG / wiki / JSON export modes

## Install

Prebuilt Linux x86_64 binary from GitHub Releases:

```sh
mkdir -p ~/.local/bin
gh release download -R EdwardAstill/convert2 --pattern 'cnv' --output ~/.local/bin/cnv
chmod +x ~/.local/bin/cnv
```

Make sure `~/.local/bin` is on your `$PATH`. The release also ships a `convert2-x86_64-linux.tar.gz` archive if you'd rather extract it yourself.

### From source (contributors only)

`cnv` bundles MuPDF and builds it from source on first compile. You need `clang` and `fontconfig` dev headers.

| Platform | Command |
| --- | --- |
| Arch Linux | `sudo pacman -S clang fontconfig` |
| Ubuntu / Debian | `sudo apt install clang libfontconfig1-dev` |
| macOS | `xcode-select --install && brew install fontconfig` |

```sh
cargo build --release
cp target/release/cnv ~/.local/bin/
```

## Usage

```sh
cnv <INPUT> [OPTIONS]
```

`INPUT` can be:

- one PDF file
- a directory of PDFs
- a quoted glob like `"papers/*.pdf"`

Main options:

| Flag | Default | Description |
| --- | --- | --- |
| `-o`, `--output <DIR>` | next to input | Output directory |
| `--min-h-gap <pts>` | `8.0` | XY-Cut horizontal-cut tuning |
| `--min-v-gap <pts>` | `12.0` | XY-Cut vertical-cut tuning |
| `--no-images` | off | Skip image extraction |
| `--hybrid <MODE>` | `off` | `off` or `docling` |
| `--hybrid-url <URL>` | `http://localhost:5001` | Hybrid backend base URL |
| `--hybrid-timeout-secs <N>` | `600` | Hybrid timeout |
| `--hybrid-policy <POLICY>` | `auto` | `auto` or `all` |
| `-v`, `--verbose` | off | Print progress to stderr |

Examples:

```sh
# One PDF
cnv paper.pdf

# Whole directory
cnv papers/ -o out/ --verbose

# Quoted glob
cnv "papers/*.pdf" -o out/

# Hybrid assist for harder pages
cnv math-paper.pdf --hybrid docling -o out/
```

## Output

For `paper.pdf`, default output looks like:

```text
paper/
  paper.md
  images/
    page1_img1.png
    page2_img1.png
```

The markdown uses standard headings, lists, fenced code blocks, GFM tables, and image references like `![image](images/page1_img1.png)`.

## Tests

```sh
cargo test
```

See [`docs/TESTING.md`](docs/TESTING.md) for the full matrix.

## Wiki

For deeper implementation notes on PDF structure, text extraction, layout recovery, tables, OCR, Markdown rendering, and evaluation, see the [`wiki/`](wiki/README.md).
