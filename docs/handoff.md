# Handoff — Pipeline Modularization, Config Extraction, Build/Flake Fixes

**Date:** 2026-09-06
**Branch:** `main` at `1367a9d` → this commit
**Status:** Big CLI/pipeline refactor committed and green: `cargo check`
(default + all-features), clippy (0 warnings), fmt, and ~276 tests all pass.

---

## This session

One commit containing two waves of work (the staged refactor alone did not
compile, so it landed together with its fixes):

| Wave | What |
| --- | --- |
| Pipeline refactor | `pipeline/mod.rs` split into stage modules; `config.rs` extraction; `render/text.rs` → `strings.rs` |
| Fixes | `sanity` field on ONNX sidecar, moved-config test imports, ETXTBSY test flake, clippy/warning cleanup |

### Verification

- `cargo check` (default and `--all-features`): clean
- `cargo clippy --all-features`: 0 warnings
- `cargo fmt --check`: clean
- `cargo test --all-features`: all suites pass (8+ consecutive clean runs)
- `cargo test --all-features --doc`: clean

---

## Pipeline refactor

`src/pipeline/mod.rs` went from a ~1,700-line monolith to an orchestrator that
delegates to four new stage modules:

| Module | Responsibility |
| --- | --- |
| `pipeline/formulas.rs` | Formula stage: sidecar construction, candidate selection, sanity gate, LaTeX recovery, debug JSON |
| `pipeline/tables.rs` | Table stage: candidates → blocks, geometry detection, debug output |
| `pipeline/media.rs` | Media stage: figure debug output, image persistence |
| `pipeline/routing.rs` | Routing stage: scan warnings, hybrid backend dispatch |

The formula sanity gate (added earlier for the sidecar quality gate) lives in
`pipeline/formulas.rs` and stamps `sidecar.sanity` with `"passed"` /
`"rejected:bad-output"` after the attempt — sidecars themselves initialize it
to `None`.

Other structural changes:

- **`src/config.rs` (new)** — CLI-free conversion configuration
  (`ConvertOptions`, OCR/formula/figure modes, `parse_formula_sidecar`,
  `FormulaSidecarArg`). `cli.rs` now only defines clap types and converts via
  `into_config()`; pipeline/layout/ocr depend on `config`, never on `cli`.
  This makes the pipeline testable without clap and keeps the binary thin.
- **`render/text.rs` → `render/strings.rs`** — renamed to reflect its actual
  scope: text normalization, escaping, heading/paragraph cleanup.
- **`formats/raw/mod.rs` deleted** — folded into `formats/mod.rs`
  (`RawFormat`: single `.md` output + image copies).
- `docs/architecture.md` updated to match the new tree.

## Fixes in this commit

1. **`formula/ocr_onnx.rs` — missing `sanity` field** (3× E0063, broke
   `--features onnx-ocr` builds). `FormulaSidecarAttempt` gained `sanity` when
   the gate landed, but the ONNX initializer wasn't updated. Fixed with
   `sanity: None` in all three arms (`Recovered`, `EmptyOutput`,
   `CommandFailed`).
2. **`tests/formula_onnx.rs` — stale imports.** Test imported
   `parse_formula_sidecar`/`FormulaSidecarArg` from `pdf_processor::cli`;
   they moved to `pdf_processor::config`.
3. **`tests/formula_ocr.rs` — flaky `subprocess_sidecar_records_empty_output`.**
   Freshly-written fake-sidecar scripts intermittently fail `execve` with
   `ETXTBSY` ("Text file busy", os error 26) on overlayfs-backed tmp. The
   retry loop now sleeps 50 ms between attempts and tries 5× (was 3
   back-to-back). Root cause is environmental (Docker/overlayfs), not
   production code.
4. **cfg-gated import hazard in `config.rs`.** `PdfpError` is only referenced
   under `#[cfg(not(feature = "onnx-ocr"))]`; the import is fully qualified
   (`crate::error::PdfpError::InvalidInput`) so both feature configurations
   compile without unused-import warnings. `cargo fix --all-features` will
   strip a plain import and break the default build — don't re-add it.
5. **Clippy**: `chunks_exact(2)` → `as_chunks::<2>()` in
   `pdf/metadata.rs` UTF-16LE decoding; unused imports removed elsewhere.

---

## Known gaps / next steps

- **No CI on push/PR.** `.github/workflows/` only has `release.yml`. The
  `onnx-ocr` feature broke silently because nothing builds it — a workflow
  running `cargo test --all-features` (or at least `--features onnx-ocr`
  check) would have caught it. This is the highest-value next item.
- Two hybrid/golden test files have ignored tests gated on fixtures; the
  `pdfium-metadata` feature needs a runtime `libpdfium` (see `docs/TESTING.md`).
- Formula quality work continues per `docs/quickstart.md` and the eval corpus
  (`scripts/quality-diff.sh`, `scripts/formula-eval.sh`).

---

## Working on this repo

- Build: `cargo build --release` (needs clang for bindgen; MuPDF bundled).
- Full gate: `cargo fmt --check && cargo clippy --all-features && cargo test --all-features`.
- Sidecar contract: `FormulaSidecar::recognize(crop) -> FormulaSidecarAttempt`
  in `formula/ocr.rs`; implementations: `SubprocessSidecar` (cmd:),
  `OnnxFormulaSidecar` (onnx:, feature `onnx-ocr`).
