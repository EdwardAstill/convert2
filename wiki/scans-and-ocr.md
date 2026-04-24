# Scans and OCR

Scanned PDFs are a separate problem, not a slightly worse text PDF.

## The distinction that matters

There are at least four useful classes:

- text-based
- scanned
- image-based
- mixed

This classification should happen early and should ideally be available per page, not only per document.

## Why scans break normal extraction

In a scan-heavy PDF, the page may contain:

- only images
- bad embedded OCR text
- no reliable Unicode mapping

In those cases, normal text extraction cannot recover meaning well enough on its own.

## The cleanest code-first strategy

For a local-first converter, the best scan strategy is usually:

1. detect scan-heavy pages or documents
2. run a local OCR preprocessing step to create a searchable derivative PDF
3. feed that derived PDF back into the normal extraction and rendering pipeline

This is better than building a completely separate converter path for scans.

## What should remain true even before OCR exists

Even without OCR, the converter should:

- correctly flag scan-heavy inputs
- avoid pretending the output is high quality
- preserve extracted images
- suggest a better route when available

`cnv` already moved in this direction by adding scan-heavy warnings and hybrid guidance.

## A good local OCR integration shape

Recommended interface:

- `--ocr`
- or `--ocr-command <CMD>`

Recommended behavior:

1. write a derived searchable PDF into a temp or work directory
2. leave the original PDF untouched
3. rerun the normal pipeline on the derived file
4. record in metadata or stderr that OCR preprocessing was used

## OCR quality pitfalls

OCR is not magic. Common problems:

- wrong language model
- poor recognition of tables and forms
- broken line grouping
- hallucinated punctuation
- loss of math and symbols

So OCR should be treated as:

- a recovery tool for unreadable scans
- not a replacement for clean text extraction on digital PDFs

## Recommended next steps for `cnv`

1. add optional local OCR preprocessing
2. keep scan detection separate from OCR execution
3. improve OCR-ish cleanup and normalization after extraction
4. measure scan improvements on the repo's scan-heavy examples

## Example files

- [Chinese scan output](../example/markdown/golden__chinese_scan/golden__chinese_scan.md)
- [Scanned example output](../example/markdown/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-09_Scanned/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-09_Scanned.md)
- [Research notes](../example/RESEARCH_NOTES.md)
