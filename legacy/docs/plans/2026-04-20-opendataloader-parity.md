# Plan: `cnv` — OpenDataLoader-class PDF → Markdown parity (Path A)

**Date:** 2026-04-20
**Author:** Edward Astill (drafted with Claude)
**Target scope:** 4–6 weeks of focused work
**Target quality:** ~75% of OpenDataLoader PDF on born-digital academic papers
**Approach:** Keep Rust + mupdf core. Port OpenDataLoader's XY-Cut++ algorithm. Fix the image-drop bug. Use Docling as an out-of-process HTTP backend for the ML-heavy parts (layout, formulas, OCR, complex tables). Add `pdfium-render` as a second PDF reader for the one thing mupdf cannot expose: font names and the PDF structure tree.

This is a path-A plan. It does not attempt to rewrite the parser, ship local ML, or produce Tagged PDF output — those are explicitly deferred. The goal is pragmatic parity on the PDFs real users bring.

---

## 1. Goals and non-goals

### Goals

1. **Fix the image-drop bug** so PDFs actually round-trip figures into the markdown output, unblocking the `IDEA.md` vision (PDF → markdown summary with figure screenshots linked).
2. **Port XY-Cut++** faithfully from OpenDataLoader's Java source, so reading order on 2-column and cross-layout pages stops zig-zagging.
3. **Integrate Docling** (`docling-serve` HTTP daemon) as an optional hybrid backend so complex pages — formula-dense, scanned, table-heavy — get ML-quality output without us writing ML.
4. **Expose font names and basic struct-tree tags** via `pdfium-render` alongside mupdf, so heading classification stops being size-only.
5. **Restructure output formats** into folders (one folder per format) to match the input-side shape and leave room to grow.
6. **Produce a rich JSON output** (a subset of the OpenDataLoader schema) so downstream consumers get bboxes, headings, tables, and figure refs.
7. **Set up a golden-output test harness** using OpenDataLoader's own sample PDFs (already copied into `papers/golden/`) and arXiv papers in `papers/` so we can measure regressions objectively.

### Non-goals (deferred)

- Tagged PDF output (WCAG AA). Requires PDFBox-equivalent writer. Defer.
- Local ML models in-process (no ONNX Runtime, no Candle). Docling covers it out-of-process.
- OmniDocBench-grade reading-order accuracy on > 3-column exotic layouts.
- Handwritten or heavily degraded scans. (Docling's OCR path handles clean scans.)
- A Rust client that directly loads the DoclingDocument Python type — we target the JSON wire format only.
- Replacing mupdf. We keep it.

### Success criteria at the end of Phase 4

1. All 13 corpus PDFs (5 OpenDataLoader samples + 8 arXiv) render to markdown without panics.
2. `attention.pdf` (Attention Is All You Need): ≥ 95% of figures linked as `.png` files next to the markdown. Section headings correct. Tables render as GitHub markdown tables.
3. `math-number-theory.pdf`: when `--hybrid docling` is used, display equations appear as `$$ ... $$` with LaTeX. When `--hybrid off`, they come through as UTF-8 text (no silent drops).
4. `chinese_scan.pdf`: with `--hybrid docling`, produces readable markdown via Docling's OCR. Without `--hybrid`, emits an explicit "scanned PDF — enable --hybrid for OCR" warning rather than a blank file.
5. Reading-order regression vs current `main`: XY-Cut++ beats the current XY-Cut on a side-by-side of `attention.pdf` page 1 (title block and abstract should precede the two-column body, not interleave with it).
6. `cargo test` passes. `cargo clippy -- -D warnings` passes. No new panics in release mode on any corpus PDF.

---

## 2. Assumptions and leveraged external tools

This plan leans heavily on existing work. We are not building from scratch.

| External artefact | Role | Source |
|---|---|---|
| OpenDataLoader Java source | Reference implementation of XY-Cut++ (we port from this, not from the paper directly) | `https://github.com/opendataloader-project/opendataloader-pdf` — cloned to `/tmp/odl-compare/opendataloader-pdf/` |
| XY-Cut++ paper (arXiv:2504.10258) | Secondary reference — guides naming and documents the theory behind the heuristics | `https://arxiv.org/abs/2504.10258` |
| OpenDataLoader sample PDFs + golden JSON | Regression fixtures (PDFUA-1 suite, arXiv papers, Chinese scan, Italian invoice) | already copied to `papers/golden/` |
| arXiv test PDFs | Diversity corpus (attention, resnet, clip, gpt3, bert, math, physics, survey) | already in `papers/` |
| Docling (`docling-project/docling`) | Layout + tables + OCR + formulas as a black box | `pip install docling` |
| docling-serve (`docling-project/docling-serve`) | HTTP wrapper for Docling, avoids per-file Python startup | `pip install "docling-serve[ui]"` or `quay.io/docling-project/docling-serve` |
| pdfium-render | Font names, struct tree, second-opinion parser | `pdfium-render = "0.9"` |
| pix2tex (optional, Phase 4+) | Per-crop math OCR fallback when Docling is not running | `pip install pix2tex` + server mode |
| `reqwest` (Rust) | HTTP client to talk to docling-serve | `reqwest = { version = "0.12", features = ["json", "multipart"] }` |
| `serde_json` (Rust) | Parse DoclingDocument JSON | already in the dep graph |
| `tempfile` (Rust) | Image crops, page PNG renders | likely already present |

### Assumptions

- The user is OK with Python being an optional runtime dependency for the hybrid path. The pure-Rust fast path keeps the "single self-contained binary" promise for the default case; `--hybrid docling` is an opt-in that requires the server.
- `docling-serve` is run by the user as a long-lived daemon (or a docker container) on `localhost`. Spinning it up per invocation negates the model-loaded-in-memory benefit.
- The user's primary test PDFs are born-digital academic papers with selectable text. Scanned documents are a secondary concern handled via Docling's OCR.

---

## 3. Current state → target state

### Current state (from `code-explorer` audit, turn 1)

```
mupdf extract → XY-cut (simple, "++" dead) → font-size classifier → markdown render
  ↓                                                                    ↓
  images collected as bboxes ──── DROPPED at main.rs:131 ──────────── never written
  equations ──── glyphs with no ToUnicode silently dropped ─────── no LaTeX, no $$
  font names ──── always "unknown" ─────────────────────────────── classifier size-only
  output formats ──── flat .rs files ──────────────────────────── no room to grow
  no JSON output, no struct-tree awareness, no hybrid fallback
```

### Target state (end of Phase 4)

```
                                        ┌── pdfium-render (font names + struct tree, opt-in per page)
                                        ↓
mupdf extract + images → XY-Cut++ → multi-signal classifier → renderer ──┬── raw/      (single .md)
  ↓                                                                      ├── rag/      (chunks)
  images decoded + saved to disk                                          ├── karpathy/ (wiki)
                                                                          ├── kg/       (JSON graph)
    ON --hybrid docling:                                                  └── json/     (DoclingDocument-subset)
    page → docling-serve HTTP → DoclingDocument JSON → merge
    (formulas as LaTeX, tables with spans, OCR for scans, figure captions)
```

---

## 4. Phased milestones

Each phase is self-contained and leaves the tree green. Each closes with an acceptance test you can run manually. No phase depends on a later one except as noted.

### Phase 0 — Foundations (est. 3–5 days)

**Why:** We cannot measure anything without a test harness. We cannot add new subsystems on top of a broken image pipeline. We cannot grow the format catalogue while it's flat files.

#### 0.1 Restructure `src/formats/` into folders

Matches the input-side shape (`src/pdf/`, `src/docx/`, …) and leaves room to add submodules (e.g. `raw/images.rs`, `raw/sections.rs`) without a re-layout later.

- `src/formats/raw.rs` → `src/formats/raw/mod.rs`
- `src/formats/rag.rs` → `src/formats/rag/mod.rs`
- `src/formats/karpathy.rs` → `src/formats/karpathy/mod.rs`
- `src/formats/kg.rs` → `src/formats/kg/mod.rs`
- Update `src/formats/mod.rs` to `pub mod raw; pub mod rag; …` (no behaviour change).

**Acceptance:** `cargo test` passes, `cargo build --release` passes, `cnv attention.pdf` still produces the same output byte-for-byte as before.

#### 0.2 Fix the PDF image-drop bug

Root cause: `src/main.rs:127-133` constructs a fresh `RawPage` with `image_refs: Vec::new()` before handing it to the classifier, discarding the `image_refs` the extractor collected.

- Stop throwing away `image_refs` in `process_pdf()`.
- In `src/pdf/extractor.rs`, actually decode each image block's bytes via mupdf's image API (we currently only record a bbox + index).
- Write PNG/JPEG files under `{output_dir}/images/page{N}_img{M}.{ext}`.
- Emit `BlockKind::Image { path }` in the classifier for each extracted image.
- Confirm the markdown renderer's `Image` arm (`src/render/markdown.rs:140`) is actually hit — it has been unreachable for PDFs until now.

**Acceptance:** running `cnv attention.pdf -o out/` produces `out/attention/images/page1_img1.png` (or similar) and the markdown contains `![image](images/page1_img1.png)` references. At least 5 of the 8 ML papers in `papers/` round-trip ≥ 1 figure.

#### 0.3 Golden-output test harness

A small `tests/golden.rs` integration test that:

1. Iterates every `.pdf` under `papers/golden/` and `papers/` (with opt-in env var for the slow corpus run).
2. Runs `cnv <pdf> -f raw -o target/golden-out/`.
3. Asserts: process exit 0, `<stem>.md` exists and is non-empty, `<stem>/images/` contains at least 1 file for PDFs we know have figures.
4. Compares output against a checked-in expected-snapshot file when one exists (under `tests/snapshots/<stem>.md`). Snapshots are generated once from the current run and diffed on subsequent runs. Use the `insta` crate or a tiny hand-rolled snapshot helper — this is not `cargo-insta`-grade yet.

This is not a correctness test yet — it's a regression harness. Correctness is a human eyeballing the snapshot diffs.

**Acceptance:** `cargo test --test golden` runs, all 13 PDFs succeed, snapshots written. Re-running produces no diff.

#### 0.4 Update `CLAUDE.md`

Remove the "images silently dropped" gotcha (now fixed). Add "Phase 0 done; see `docs/plans/2026-04-20-opendataloader-parity.md` for the remaining roadmap."

---

### Phase 1 — XY-Cut++ port (est. 1–2 weeks)

**Why:** Reading order on multi-column pages is the single most visible quality gap vs OpenDataLoader. It is also a pure-algorithmic change with no new runtime dependency.

#### 1.1 Port the Java implementation

Reference: `/tmp/odl-compare/opendataloader-pdf/java/opendataloader-pdf-core/src/main/java/org/opendataloader/pdf/processors/readingorder/XYCutPlusPlusSorter.java` (651 lines).

The OpenDataLoader Java impl is a **pragmatic simplification** of the arXiv paper. We port the Java version first because it is the version the benchmark-winning product actually uses. Paper-faithful mode is optional future work.

Rewrite `src/layout/xycut.rs` around this API:

```rust
pub struct XyCutParams {
    pub beta: f64,                       // default 2.0
    pub density_threshold: f64,          // default 0.9
    pub overlap_threshold: f64,          // default 0.1
    pub min_overlap_count: usize,        // default 2
    pub min_gap_threshold: f64,          // default 5.0 (PDF points)
    pub narrow_element_width_ratio: f64, // default 0.1
}

pub fn xy_cut_plus_plus(
    items: &[Item],
    params: &XyCutParams,
) -> Vec<usize>;
```

Four internal stages:

1. **Cross-layout pre-masking** — width ≥ `beta × maxWidth` and ≥ 2 horizontal overlaps (10% of min-width). Requires ≥ 3 objects.
2. **Density ratio** — computed but used only as a gap-tie tiebreaker. Kept for parity and for future paper-mode.
3. **Recursive segmentation** — pick the axis with the larger gap (≥ 5pt); on degenerate splits, fall back to `(y0, x0)` sort. Implement the **narrow-outlier retry**: if the vertical gap is under the threshold, filter items whose width is < 10% of region width and retry the scan — this is what rescues 2-column pages with full-width page numbers.
4. **Merge back cross-layout elements** — simple Y-merge (no IoU, no label priorities). Cross wins on tie.

**Critical coordinate-flip note:** the Java impl uses PDF native coordinates (y grows upward, `topY > bottomY`). Our `Bbox` uses top-left origin (y grows downward, `y0 < y1`). Every comparison involving Y flips. The research agent flagged this as the #1 porting bug source.

#### 1.2 Test coverage

- Unit tests in `src/layout/xycut.rs::tests` covering:
  - Two-column page with body text only (both columns sorted correctly, column gutter detected).
  - Two-column page with a title spanning both columns (title pre-masked, placed above body flow).
  - Two-column page with a full-width figure in the middle (figure placed correctly, not mixed into one column).
  - Page with a full-width footer (page number) that would otherwise defeat column detection (narrow-outlier retry kicks in).
  - Single-column page (no spurious column cut).
- Snapshot test comparing reading order of `attention.pdf` page 1 before vs after. Expect the abstract to come before the two-column body.

#### 1.3 Remove the dead `merge_fragmented_words` code

It has been `#[allow(dead_code)]` for a while. Delete or integrate as a pre-pass (the OpenDataLoader port does not need it; text runs from mupdf are already coherent at the block level).

**Acceptance:**
- All unit tests pass.
- `attention.pdf` page 1 ordering: title → authors → abstract → intro-col-1 → intro-col-2, **not** interleaved.
- `resnet.pdf`, `bert.pdf`, `clip.pdf` page 1 similarly correct.
- Golden snapshots updated (expect large, explicable diffs).

---

### Phase 2 — Docling hybrid backend (est. 2 weeks)

**Why:** This is the big quality unlock. One HTTP call gives us formula LaTeX, tables with row/column spans, OCR for scans, and higher-quality layout. It is also the feature most aligned with the user's IDEA.md goal (summarising PDFs, including equations).

#### 2.1 New module: `src/hybrid/`

```
src/hybrid/
├── mod.rs           — public API
├── client.rs        — reqwest client for docling-serve
├── triage.rs        — decide which pages go to Docling
├── transform.rs     — DoclingDocument JSON → our Document/Block types
├── merge.rs         — Docling output merged with local mupdf extraction
└── types.rs         — serde-derive structs mirroring the DoclingDocument wire schema (subset)
```

#### 2.2 Client wiring

- CLI: add `--hybrid <mode>` with `off | docling` (default `off`); `--hybrid-url <url>` (default `http://localhost:5001`).
- `client.rs`:
  ```rust
  pub struct DoclingClient { base_url: String, http: reqwest::Client }
  impl DoclingClient {
      pub async fn convert_file(&self, pdf: &Path, options: &DoclingOptions)
          -> Result<DoclingDocument, DoclingError>;
  }
  ```
- Post to `POST /v1/convert/file` as multipart. Body options: `{"to_formats":["json"], "do_ocr":true, "do_table_structure":true, "do_formula_enrichment":true}`.
- Response: parse `ConvertDocumentResponse.document` (a `DoclingDocument`).
- Retry with exponential backoff on 5xx; fail fast on 4xx.
- Bring in `tokio` only if not already present; use `reqwest::blocking` if we want to avoid introducing tokio (simpler, matches the mupdf sequential loop). **Use blocking.**

#### 2.3 Triage (which pages go to Docling?)

OpenDataLoader triages at page granularity — simple pages stay local, complex pages go to the AI backend. We do the same, initially with crude heuristics:

- Page has ≥ 1 formula-looking block (heuristic: has ≥ 4 Unicode math symbols from a curated list: `∫∑∏∞≤≥≠∈∉⊂⊃∩∪∀∃∅ℕℤℚℝℂ∂∇√±×·` etc.) → route to Docling.
- Page has a table-looking region (≥ 2×2 block grid, detected by the existing table pass) → route to Docling.
- Page has < 100 extractable Unicode code points per 100 sq. in. → likely scanned, route to Docling.
- Everything else → stay local.

In Phase 2 the triage is a simple "page-level decision" function. Future work: crop-level (send only the formula crop, not the whole page).

#### 2.4 Transform and merge

- `transform.rs`: convert `DoclingDocument`'s items (`TitleItem`, `SectionHeaderItem`, `TextItem`, `ListItem`, `TableItem`, `PictureItem`, `FormulaItem`, `CodeItem`) into our `BlockKind`s.
- Add new variants to `BlockKind` as needed:
  - `Formula { latex: String, display: bool }`
  - `Table { rows: Vec<TableRow> }` with cells carrying `row_span`, `col_span` (the existing `TableCell` is flat).
  - `Figure { path: PathBuf, caption: Option<String> }` (currently we only have `Image`).
- `merge.rs`: for pages routed to Docling, replace the local `Document.pages[i]` with the transformed Docling output. Preserve page numbers. If Docling fails for a page, log and fall back to the local extraction (never lose data).

#### 2.5 Renderer updates

`src/render/markdown.rs`:
- `Formula { latex, display: true }` → `\n\n$$ {latex} $$\n\n`
- `Formula { latex, display: false }` → `${latex}$`
- `Table` with spans → still render as GFM markdown but document the lossiness in a comment above the table (GFM has no row/column spans).
- `Figure { path, caption: Some(c) }` → `![image]({path})\n\n*{c}*\n\n`

#### 2.6 Docs

- `docs/hybrid-docling.md` — how to set up `docling-serve` (pip install, docker, environment, API key). Tell the user it is optional, cite the quality numbers, show the CLI flags.

#### 2.7 Test coverage

- Integration test with a stubbed HTTP server (mockito or `httpmock` crate) returning a canned DoclingDocument JSON. Assert that the transform produces the expected blocks.
- A ℹ️-tagged test (env-gated: `DOCLING_URL=http://localhost:5001 cargo test hybrid_live`) that hits a real Docling server and asserts ≥ 1 formula is extracted from `math-number-theory.pdf`.
- Regression: `attention.pdf` with `--hybrid off` produces identical output to the Phase 1 snapshot.

**Acceptance:**
- `cnv math-number-theory.pdf --hybrid docling --hybrid-url http://localhost:5001 -o out/` produces markdown with `$$ ... $$` LaTeX for display equations.
- `cnv chinese_scan.pdf --hybrid docling` produces readable (OCR'd) text rather than a silent failure.
- `--hybrid off` is byte-for-byte identical to the pre-Phase-2 output.
- Docling-server unreachable is a graceful error message, not a panic.

---

### Phase 3 — pdfium-render metadata layer (est. 1–2 weeks)

**Why:** Font-size-only heading detection fails on documents with uniform body font. `pdfium-render` gives us font family, weight, and the struct tree — the other signals OpenDataLoader's classifier uses.

This phase is smaller than Phase 2 and strictly additive. If Phase 2 takes longer than expected, this can slip.

#### 3.1 Add `pdfium-render` as a dep

```toml
pdfium-render = { version = "0.9", features = ["thread_safe", "image"] }
```

Dynamic-load Pdfium .so via `Pdfium::bind_to_system_library()`. Document in `CLAUDE.md` that the user needs `libpdfium` installed at runtime (apt, brew, or `pdfium-binaries`).

#### 3.2 Per-page metadata sidecar

New module `src/pdf/metadata.rs`:

```rust
pub struct PageMetadata {
    pub fonts_by_bbox: HashMap<BboxKey, FontInfo>, // keyed by rounded bbox
    pub struct_tree_tags: Vec<StructTag>,          // if tagged PDF
}

pub struct FontInfo { pub family: String, pub weight: u16, pub italic: bool }
pub struct StructTag { pub bbox: Bbox, pub role: String, pub alt: Option<String> }
```

Populated by opening the PDF a second time with `pdfium-render`, per-page. Runs only once per file (O(pages) work).

#### 3.3 Classifier upgrades

In `src/layout/classifier.rs`:
- Heading detection: use (size ratio) OR (weight ≥ 700 bold) OR (struct-tree role in {H1..H6}). Weight by rarity — a block whose font is used < 5% of the document is a stronger heading signal than a common one.
- Add `BlockKind::Figure` vs `BlockKind::Image` split using struct-tree `Figure` role when present.
- Figure captions become linked to the figure via struct-tree proximity when available.

#### 3.4 Test coverage

- Add a PDF with tagged structure (pdfua-1-reference-suite has tagged PDFs) to the fixtures.
- Unit test: the classifier picks up `H1` struct-tree tags even when font size matches body.
- Regression: on untagged PDFs, behaviour is unchanged.

**Acceptance:**
- `papers/golden/pdfua-1-reference-suite-1-1/*.pdf` — heading structure is correctly extracted from the struct tree.
- `attention.pdf` (untagged) — no regression.

---

### Phase 4 — JSON schema output, polish, docs (est. 1 week)

#### 4.1 New output format: `json`

`src/formats/json/mod.rs`. Schema is a subset of OpenDataLoader's, with DoclingDocument-style naming:

```json
{
  "source": { "path": "...", "pages": 42 },
  "pages": [{
    "page_no": 1,
    "size": { "width": 612, "height": 792 },
    "items": [
      { "id": "p1-t0", "type": "title", "text": "...", "bbox": [l, t, r, b] },
      { "id": "p1-h1-0", "type": "section_header", "level": 1, "text": "...", "bbox": [...] },
      { "id": "p1-p0", "type": "paragraph", "text": "...", "bbox": [...] },
      { "id": "p1-fig0", "type": "picture", "path": "images/page1_img1.png", "caption": "...", "bbox": [...] },
      { "id": "p1-eq0", "type": "formula", "latex": "...", "display": true, "bbox": [...] },
      { "id": "p1-tbl0", "type": "table", "rows": [[{ "text": "...", "row_span": 1, "col_span": 1 }]] }
    ]
  }]
}
```

Update `InputType::supports_format()` in `src/cli.rs` to include the new format. Add a `Format::Json` variant.

#### 4.2 Docs pass

- `docs/architecture.md` — current pipeline deep-dive + gotchas (supersedes the inline comments in `CLAUDE.md`).
- `docs/pdf-internals.md` — normal-prose primer on what a PDF actually is (objects, content streams, `Tj/TJ`, fonts, `ToUnicode`, CID, vector paths, struct tree) and which parts `cnv` touches. User explicitly asked for this.
- `docs/hybrid-docling.md` (from Phase 2) — operator's guide to the Docling sidecar.
- Update `README.md` with a feature matrix and a demo GIF or screenshot.

#### 4.3 CLI polish

- `--verbose` shows per-page triage decisions in hybrid mode.
- `--dry-run` prints what would be produced without writing files.
- Progress bar for multi-file batches.

#### 4.4 Golden-diff acceptance

Regenerate all snapshots. Human-eyeball-review each diff. Commit.

**Acceptance:**
- `cnv attention.pdf -f json -o out/` produces valid JSON that deserialises with `serde_json`.
- All docs exist and render correctly on GitHub.
- The whole 13-PDF corpus processes clean in < 5 minutes without Docling, < 15 minutes with Docling on CPU (scan-bound).

---

## 5. Leveraged external tools summary

| Phase | Tool | How used |
|---|---|---|
| 0 | OpenDataLoader samples (copied to `papers/golden/`) | Test fixtures |
| 0 | `insta` (or hand-rolled) | Snapshot testing |
| 1 | OpenDataLoader Java XY-Cut++ source | Port reference |
| 1 | arXiv:2504.10258 | Secondary reference |
| 2 | `docling-serve` Docker image | HTTP backend |
| 2 | `reqwest` + `serde_json` | Rust HTTP client |
| 2 | `httpmock` or `mockito` | Test double |
| 3 | `pdfium-render` + `pdfium-binaries` | Font names + struct tree |
| 4 | (no new tools) | Polish |

---

## 6. Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Docling's wire schema changes in a minor release and breaks our parser | Medium | Medium | Pin to a Docling version in `docs/hybrid-docling.md`; keep `transform.rs` tolerant of unknown fields (`#[serde(flatten)]` catch-alls). |
| XY-Cut++ Rust port drifts from Java due to coord-system mistakes | High | Medium | Write unit tests against hand-constructed bboxes that match the Java test expectations. Coord-flip every comparison; add a comment box at the top of `xycut.rs` with the flip table. |
| `pdfium-render` dynamic load fails on the user's system | Low | Low | Keep it behind a Cargo feature flag (`--features pdfium-metadata`). Classifier falls back to size-only when the feature is off. |
| Docling startup on CPU is too slow for iteration | Medium | Low | Document "run as Docker with `-p 5001:5001` once, leave it running". Cold-start only matters on first invocation. |
| Image decode via mupdf's API is more involved than expected | Medium | High | If mupdf's image API is awkward, fall back to rendering the image's page region to PNG via `pixmap` (lossy but always works). |
| Python sidecar goes stale and duplicates Rust logic | Medium | Low | Delete `python/convert2_pdf.py` once Phase 0 lands — it was a stopgap for the image-drop bug. |
| We break existing RAG / karpathy / kg output formats during restructure | Low | Medium | Phase 0 includes byte-identical regression check before/after folder rename. |
| Users want Tagged PDF output (deferred) | Low | Low | Documented as a non-goal. Revisit if a real user asks. |

---

## 7. Test corpus

Already in place under `papers/`:

| File | Profile | Purpose |
|---|---|---|
| `attention.pdf` | 2-col ML, figures, tables, inline math | Canonical smoke test |
| `resnet.pdf` | 2-col ML, tables, figures | Table stress |
| `bert.pdf` | 2-col ML, moderate math | Baseline |
| `clip.pdf` | 2-col ML, many figures | Image-extraction stress |
| `gpt3.pdf` | Long paper (60+ pages) | Large-file / memory stress |
| `math-number-theory.pdf` | Dense display equations | Formula stress |
| `physics-hep.pdf` | Math + physics notation | Formula stress |
| `survey-llm.pdf` | Figure-heavy survey | Image count stress |
| `golden/lorem.pdf` | Trivial text | Happy path |
| `golden/1901.03003.pdf` | arXiv layout sample | OpenDataLoader reference |
| `golden/2408.02509v1.pdf` | arXiv layout sample | OpenDataLoader reference |
| `golden/chinese_scan.pdf` | Scanned, Chinese | OCR path (Docling only) |
| `golden/issue-336-conto-economico-bialetti.pdf` | Italian invoice | Real-world noise |
| `golden/pdfua-1-reference-suite-1-1/*.pdf` | Tagged PDFs | Struct-tree validation |

Add more if a specific bug demands it.

---

## 8. Out-of-scope (explicitly listed so we don't creep)

- Tagged PDF **writing** (emitting a `/StructTreeRoot` in the output PDF). OpenDataLoader does this; we do not.
- Local ML inference (ONNX / Candle). Docling covers it.
- Handwritten math. `pix2tex` and `UniMERNet` are named but not integrated in this plan; only pencilled in as Phase 4+ add-ons.
- GUI / web UI. CLI only.
- Other input types (DOCX, EPUB, PPTX, HTML). The PDF-only focus is intentional; other extractors already work and are not the weak link.
- Parallel / rayon page processing (mupdf's thread safety blocks this, and Docling already parallelises server-side for us).
- Paper-mode XY-Cut++ (β=1.3, median-width, IoU-weighted merge, label priorities). Java-mode is enough for Phase 1; paper-mode is a future nice-to-have.

---

## 9. Execution order and parallelism

Linear order is safest. If two people work on this:

- **Lane A (sequential):** Phase 0 → Phase 1 → Phase 2. Each phase depends on the previous for its regression baseline.
- **Lane B (can start after Phase 0):** Phase 3 (pdfium-render) can happen in parallel with Phase 2, since they touch different modules.
- **Phase 4** requires Phases 2 and 3 to be landed for the JSON output to be meaningful.

---

## 10. Open questions for the user

1. **Python as a dependency.** Confirmed OK for the hybrid path? (The default, `--hybrid off`, stays zero-Python.)
2. **Docling deployment.** Do you want the plan to include a Dockerfile / docker-compose that runs `docling-serve` alongside `cnv`? Or leave it as "user's problem"?
3. **Namespace.** Should the CLI rename to something other than `cnv` for the hybrid-capable release, or is the existing name fine?
4. **Output folder layout.** Currently each PDF gets `out/<stem>.md`. Should images go in `out/<stem>/images/` (nested) or `out/images/<stem>/` (flat)? Nested is my default in the plan.
5. **Tagged PDF output** — really deferred forever, or a Phase 5 follow-up?

---

## 11. Summary

We do not rewrite. We port one well-understood algorithm, fix one bug, and wrap one external service. That is the whole plan. In return we get reading order that survives two-column layouts, markdown that contains images, and a fallback path for the content mupdf cannot read on its own (formulas, complex tables, scans). The work is 4–6 weeks for one focused engineer and every phase leaves the tree green.

The single most valuable hour in the plan is **Phase 0.2** — fixing the image-drop bug. It unblocks `IDEA.md` and is a one-sitting change.
