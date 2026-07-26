# CLI and Processors (`src/cli.rs`, `src/commands.rs`, `src/processor/`)

The CLI layer defines all command-line arguments, types, and defaults. The processor directory implements each command.

## Source files

| File | Purpose |
|---|---|
| `src/cli.rs` | Clap CLI definition: `Cli`, `Command`, `ConvertArgs`, `TableMode`, `FormulaMode`, CLI defaults |
| `src/commands.rs` | `AppCommand` → processor dispatch (`process_convert()`, `process_ocr()`, ...) |
| `src/batch.rs` | Input resolution: file, directory, or glob → `Vec<PathBuf>` |

### Processors (`src/processor/`)

| File | Command | Purpose |
|---|---|---|
| `mod.rs` | — | Dispatch, common helpers |
| `inspect.rs` | `pdfp inspect` | Page metadata, scan detection, geometry, text density |
| `search.rs` | `pdfp search` | Embedded text search with page reporting, optional OCR |
| `metadata.rs` | `pdfp metadata` | PDF info dictionary: show, set, clear; date handling; signed-PDF safety |
| `ocr_cmd.rs` | `pdfp ocr` | Standalone OCRmyPDF command |
| `pages.rs` | `pdfp pages` | Page operations: extract, delete, split, reorder, merge |
| `page_range.rs` | — | Page range parsing: `1`, `1-3`, `odd`, `even`, `all` |
| `impose.rs` | `pdfp impose` | Imposition: 2-up, booklet |
| `resize.rs` | `pdfp page resize`, `pdfp page crop` | Page resize and CropBox editing |
| `text.rs` | `pdfp page text` | Searchable text overlays with font, colour, page, and position controls |
| `doctor.rs` | `pdfp doctor` | Runtime dependency check (OCR availability, version) |
| `update.rs` | `pdfp update` | Self-update: check, download, install |

## Key types

| Type | File | Purpose |
|---|---|---|
| `Cli` | `cli.rs` | Root struct: `Command` enum |
| `Command` | `cli.rs` | Enum: `Convert`, `Ocr`, `Doctor`, `Inspect`, `Metadata`, `Search`, `Eval`, `Pages`, `Impose`, `Page`, `Update` (`Convert` is default) |
| `ConvertArgs` | `cli.rs` | All conversion options: table mode, formula mode, figure mode, OCR mode, hybrid config, markdown style |
| `TableMode` | `cli.rs` | `Auto`, `Native`, `Layout`, `Off` |
| `FormulaMode` | `cli.rs` | `Auto`, `Local`, `Hybrid`, `Off` |
| `FormulaEmitMode` | `cli.rs` | `Conservative`, `Auto`, `All`, `None` |
| `InspectOutput` | `inspect.rs` | Structured JSON output from inspect |
| `SearchOutput` | `search.rs` | Structured JSON output from search |
| `MetadataOutput` | `metadata.rs` | Structured metadata display |
| `OcrCommandResult` | `ocr_cmd.rs` | OCR command result with provenance |
| `PageRange` | `page_range.rs` | Parsed range: `Single(n)`, `Range(a,b)`, `All`, `Odd`, `Even` |
| `ResizeTarget` | `resize.rs` | Paper size target (`A4`, `Letter`, custom dimensions) |
| `FitMode` | `resize.rs` | `Contain`, `Cover`, `Stretch` |

## CLI defaults

| Flag | Default | Notes |
|---|---|---|
| `--markdown-style` | `clean` | Reader-friendly reflowed Markdown |
| `--tables` | off | Also save detected table crops under `tables/` |
| `--table-mode` | `auto` | Hidden/debug table rendering mode |
| `--formulas` | `auto` | High-confidence candidates as display math |
| `--images` | off | Also save detected figure/image crops under `images/` |
| `--figures` | unset | Hidden/debug image source mode; default asset mode is snapshots |
| `--ocr` | `auto` | OCR scan-heavy pages only |
| `--hybrid` | `off` | No Docling enrichment |
| `--min-h-gap` | `8.0` | XY-Cut horizontal threshold |
| `--min-v-gap` | `12.0` | XY-Cut vertical threshold |

Conservative mode overrides: `--table-mode layout`, `--formulas auto` (but no heuristic rendering), `--formula-emit conservative`, `--figures embedded`, disables figure snapshots.

## Command dispatch flow

```
main.rs::main()
  → cli.rs::Cli::parse()
    → commands.rs::run_command(AppCommand::Convert(args))
      → batch.rs::resolve_input(args.input) → Vec<PathBuf>
        → For each PDF:
          → ocr::prepare_pdf()          // optional OCR preprocessing
            → pipeline::run_pipeline()   // extract → layout → detect → classify → merge
              → render::render_document() // Markdown output
                → write output files
```

## Cross-references

- `docs/CLI.md` — full CLI reference with examples
- `docs/TESTING.md` — processor smoke test commands
- [pipeline.md](pipeline.md) — pipeline orchestration
- [ocr-preprocessing.md](ocr-preprocessing.md) — OCR command
- [hybrid-backend.md](hybrid-backend.md) — hybrid configuration
