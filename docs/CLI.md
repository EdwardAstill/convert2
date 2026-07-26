# PDF Processor CLI Guide

`pdf-processor` is a command-line tool. The installed command is `pdfp`.

Use `pdfp --help` to see the command tree, and use `--help` on any command or subcommand to see its flags:

```sh
pdfp --help
pdfp convert --help
pdfp ocr --help
pdfp doctor --help
pdfp inspect --help
pdfp metadata set --help
pdfp search --help
pdfp eval --help
pdfp pages --help
pdfp pages extract --help
pdfp pages rotate --help
pdfp impose booklet --help
pdfp page resize --help
pdfp page crop --help
pdfp page text --help
```

## Install

Install the latest Linux release with one command:

```sh
curl -fsSL https://github.com/EdwardAstill/pdf-processor/releases/latest/download/install.sh | sh
```

The installer places `pdfp` under `~/.local/share/pdfp`, symlinks it into `~/.local/bin`, and installs OCR dependencies with the platform package manager when they are missing. On Arch-like systems, it uses `yay` or `paru` for OCRmyPDF because OCRmyPDF is distributed through AUR. Set `PDFP_INSTALL_OCR=0` to skip OCR dependency installation.

Confirm the binary is available:

```sh
pdfp --version
pdfp --help
pdfp doctor
```

## Mental Model

`pdfp` is one CLI with several PDF workflows:

| Workflow | Command | Output |
| --- | --- | --- |
| Convert PDF to Markdown | `pdfp convert input.pdf` | Markdown folder |
| Create a searchable OCR PDF | `pdfp ocr input.pdf -o output.pdf` | New searchable PDF |
| Check runtime dependencies | `pdfp doctor` | Human summary or JSON |
| Inspect PDF pages | `pdfp inspect input.pdf` | Human summary or JSON, optionally OCR-assisted |
| Read/write document metadata | `pdfp metadata ...` | Human summary, JSON, or new PDF |
| Search embedded text | `pdfp search input.pdf "needle"` | Matching pages or JSON, optionally OCR-assisted |
| Evaluate extraction quality | `pdfp eval fixtures/` | Formula, heading, table, and image metrics |
| Extract/delete/split/reorder/merge/rotate pages | `pdfp pages ...` | New PDF files |
| Create 2-up or booklet layouts | `pdfp impose ...` | New PDF files |
| Resize, crop, or add text to pages | `pdfp page ...` | New PDF file |

The old shorthand still works:

```sh
pdfp input.pdf -o out/
```

That is the same as:

```sh
pdfp convert input.pdf -o out/
```

## Convert to Markdown

Convert one PDF:

```sh
pdfp convert paper.pdf -o out/
```

Convert every PDF in a directory:

```sh
pdfp convert papers/ -o out/ --verbose
```

Convert a shell glob. Quote the glob so `pdfp` can resolve it consistently:

```sh
pdfp convert "papers/*.pdf" -o out/
```

Useful conversion flags:

| Flag | Meaning |
| --- | --- |
| `-o`, `--output <DIR>` | Output directory |
| `--images` | Also save detected figures/images under `images/` |
| `--tables` | Also save detected table crops under `tables/` |
| `--equations` | Also save detected equation crops under `equations/` |
| `--pages <RANGE>` | Convert selected pages, e.g. `1-3,9`, `odd`, or `all` |
| `--ocr auto|force|off` | OCR preprocessing mode; default is `auto` |
| `--lang <LANGS>` | OCR languages, such as `eng` or `eng+deu` |
| `-v`, `--verbose` | Print progress to stderr |

Default output for `paper.pdf`:

```text
paper.md
```

With optional assets:

```sh
pdfp paper.pdf --images --tables --equations
```

```text
paper.md
images/
tables/
equations/
```

For a single input, `-o out/` writes `out/paper.md` plus any requested asset folders. For a directory or glob, `pdfp` keeps one subdirectory per input under `out/` to avoid asset filename collisions.

The default conversion path uses the best local extraction system: clean Markdown rendering, automatic scan triage, OCR when needed, table reconstruction, and formula detection. The asset flags do not turn those systems on; they only ask `pdfp` to also save visual crops.

## Local OCR

Markdown conversion uses `--ocr auto` by default. It never edits the input PDF in place. When OCR is needed, `pdfp` writes a searchable derivative PDF to a temporary or cache directory, then runs the normal conversion, inspection, or search path against that derivative while keeping output names based on the original file.

Use `--lang` when the scan is not English:

```sh
pdfp scan.pdf --lang eng+deu
```

Force OCR when the embedded text layer exists but is damaged:

```sh
pdfp bad-text-layer.pdf --ocr force --lang eng+deu
```

Inspect the OCR decision:

```sh
pdfp inspect scan.pdf --ocr auto --json
```

Search a scan through the OCR sidecar:

```sh
pdfp search scan.pdf "needle" --ocr auto --json
```

Local OCR calls OCRmyPDF, so the machine also needs Tesseract and any requested language packs. If `--ocr auto` is used on a clean born-digital PDF, OCR is skipped and missing OCR tools do not matter. If a scan-heavy PDF needs OCR and the command is missing, `pdfp` exits with a message naming the missing OCRmyPDF command.

`pdfp` resolves OCRmyPDF in this order:

1. `--command <PATH>` / `--ocr-command <PATH>`
2. `PDFP_OCR_COMMAND`
3. `tools/ocr/ocrmypdf` bundled next to the installed `pdfp`
4. `ocrmypdf` from `PATH`

Check the runtime setup:

```sh
pdfp doctor
pdfp doctor --json
```

### Standalone OCR PDF

Use `pdfp ocr` when the desired output is a searchable PDF rather than Markdown:

```sh
pdfp ocr scan.pdf -o scan.searchable.pdf --mode auto --lang eng
```

Useful standalone OCR flags:

| Flag | Meaning |
| --- | --- |
| `-o`, `--output <PDF>` | Output searchable PDF |
| `--mode auto|force` | Skip readable PDFs/pages or force raster OCR |
| `--lang <LANGS>` | OCR languages, such as `eng`, `eng+deu`, or `chi_sim` |
| `--cache-dir <DIR>` | Reuse OCRmyPDF sidecar results before copying to output |
| `--timeout-secs <N>` | OCR timeout |
| `--command <PATH>` | OCRmyPDF command path |
| `--json` | Emit OCR decision/provenance JSON |

`--mode auto` first checks whether the PDF already has readable text. If OCR is not needed, `pdfp` copies the input to the output path and reports `status: skipped` in JSON. `--mode force` sends every page through OCRmyPDF and is the right option only when the embedded text layer is damaged.

## Inspect PDFs

Print a readable summary:

```sh
pdfp inspect input.pdf
```

Print JSON for scripts:

```sh
pdfp inspect input.pdf --json
```

Use this before OCR or page editing to answer:

- How many pages does this PDF have?
- What are the page sizes?
- Does it look scan-heavy?
- Which pages have readable embedded text?

## Document Metadata

Show document information metadata:

```sh
pdfp metadata show input.pdf
pdfp metadata show input.pdf --json
```

Set fields and write a new PDF:

```sh
pdfp metadata set input.pdf -o output.pdf \
  --title "Revised Report" \
  --author "Engineering Team" \
  --subject "Metadata audit" \
  --keywords "pdf,metadata"
```

Clear fields and write a new PDF:

```sh
pdfp metadata clear input.pdf -o output.pdf --fields title,author
pdfp metadata clear input.pdf -o output.pdf --fields all
```

Supported fields:

| Field | Set flag | Clear value |
| --- | --- | --- |
| Title | `--title <TEXT>` | `title` |
| Author | `--author <TEXT>` | `author` |
| Subject | `--subject <TEXT>` | `subject` |
| Keywords | `--keywords <TEXT>` | `keywords` |
| Creator | `--creator <TEXT>` | `creator` |
| Producer | `--producer <TEXT>` | `producer` |
| Creation date | `--creation-date <DATE>` | `creation-date` |
| Modification date | `--mod-date <DATE>` | `mod-date` |

Dates accept `now`, RFC3339 such as `2026-05-19T12:30:00Z`, or raw PDF date syntax such as `D:20260519123000Z` and `D:20260519123000+08'00'`. `metadata set` automatically updates `ModDate` when changing another field; add `--no-touch-mod-date` to leave it unchanged.

`pdfp metadata` edits the PDF document information dictionary. If the file also has XMP metadata, the command preserves XMP and reports a warning because XMP can still contain older values. PDFs with signature fields are refused by default; `--force-signed` allows writing a new file when you accept that signatures may be invalidated. Encrypted/password-protected PDFs are refused because rewriting them could remove encryption or produce an incomplete file; decrypt a copy first, for example with `qpdf --decrypt input.pdf decrypted.pdf`.

## Search Embedded Text

Search all pages:

```sh
pdfp search input.pdf "Fourier"
```

Search selected pages:

```sh
pdfp search input.pdf "Fourier" --pages 1-10
```

Return JSON:

```sh
pdfp search input.pdf "Fourier" --json
```

By default, search only sees text embedded in the PDF. Add `--ocr auto` to let scan-heavy PDFs be converted to a searchable derivative first, or `--ocr force` when an existing text layer is unusable.

## Evaluate Quality

Run the local conversion pipeline against fixture JSON files and report recall,
precision, false-positive counts, and image/figure retention:

```sh
pdfp eval tests/eval_fixtures/
```

Each fixture JSON names a PDF and the expected emitted formula count, formula detection count, optional LaTeX snippets, headings, table
presence/regions, and image/figure expectations for selected pages. Missing PDFs
are skipped with a clear message, which keeps large local corpora out of the
repository while preserving the evaluation contract. See
`tests/eval_fixtures/README.md` for the schema and Stage 8/9 benchmark notes.

## Page Operations

Page ranges are 1-indexed. Supported forms:

| Selection | Meaning |
| --- | --- |
| `1` | Page 1 |
| `1-3` | Pages 1 through 3 |
| `1,3,5-7` | Pages 1, 3, 5, 6, 7 |
| `odd` | Odd-numbered pages |
| `even` | Even-numbered pages |
| `all` | All pages |

Extract pages:

```sh
pdfp pages extract input.pdf --pages 1-3,9 -o excerpt.pdf
```

Delete pages:

```sh
pdfp pages delete input.pdf --pages 2,4-6 -o edited.pdf
```

Split into chunks:

```sh
pdfp pages split input.pdf --every 10 -o chunks/
```

Reorder pages:

```sh
pdfp pages reorder input.pdf --pages 1,3,2,4-8 -o reordered.pdf
```

Merge PDFs:

```sh
pdfp pages merge one.pdf two.pdf three.pdf -o merged.pdf
```

Set page rotation on selected pages:

```sh
pdfp pages rotate input.pdf --pages 1,3 --degrees 90 -o rotated.pdf
```

These commands write new files. They do not edit the input PDF in place.

## Imposition and Page Editing

Put two source pages onto each output page:

```sh
pdfp impose 2up input.pdf -o two-up.pdf
```

Create booklet spread order:

```sh
pdfp impose booklet input.pdf -o booklet.pdf
```

Resize pages to A4:

```sh
pdfp page resize input.pdf --paper a4 --fit contain -o resized.pdf
```

Set CropBox on selected pages:

```sh
pdfp page crop input.pdf --pages all --box 0 0 500 700 -o cropped.pdf
```

Add a searchable text overlay to selected pages:

```sh
pdfp page text input.pdf -o labelled.pdf \
  --pages 1,3 \
  --text "APPROVED" \
  --x 36 --y 36 --origin top-left \
  --font helvetica-bold --font-size 18 \
  --colour '#cc0000'
```

Text positions are baseline coordinates in PDF points, relative to the active
CropBox (or MediaBox when there is no CropBox). With the default
`--origin bottom-left`, X grows right and Y grows up. With
`--origin top-left`, X grows right and Y grows down. Coordinates describe the
page user space before any page-level `/Rotate` is applied.

The built-in fonts are Helvetica, Times, and Courier, including their bold and
italic/oblique variants; run `pdfp page text --help` for the exact names. These
standard PDF fonts support Windows-1252 text. Colours accept common names such
as `red`, `blue`, and `gray`, or `#RGB`/`#RRGGBB`. Newlines create multiple
lines; `--line-height` controls the baseline spacing.

Supported page-editing options:

| Flag | Values |
| --- | --- |
| `--paper` | `a4`, `letter` |
| `--fit` | `contain`, `cover`, `stretch` |
| `--pages` | `1`, `1-3`, `odd`, `even`, `all` for crop or text |
| `--box` | `x0 y0 x1 y1` in PDF points |
| `--x`, `--y` | Text baseline position in PDF points |
| `--origin` | `bottom-left`, `top-left` |
| `--font` | Built-in Helvetica, Times, or Courier variant |
| `--font-size` | Positive size in PDF points |
| `--color`, `--colour` | Named colour, `#RGB`, or `#RRGGBB` |
| `--line-height` | Optional multiline baseline spacing |

## Safety and Limits

- `pdfp` is local-first. Normal conversion, search, inspection, and page operations do not require a network service.
- `--ocr auto` and `--ocr force` require OCRmyPDF plus Tesseract only when OCR is actually run.
- `--hybrid docling` requires a separate Docling server.
- Page editing and metadata write commands write new PDFs and refuse to use the input path as the output path.
- Text overlays are appended as searchable page content. Encrypted PDFs are refused, and signed PDFs require `--force-signed` because content changes can invalidate signatures.
- Search uses embedded PDF text unless local OCR is explicitly requested.
- Dedicated metadata commands update Info dictionary fields only. They do not synchronize XMP packets.
- Merge/reorder/imposition preserve page contents conservatively, but document-level metadata, outlines, forms, and annotations are not yet guaranteed.
