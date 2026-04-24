# Fix Plan

This plan is updated from the **current** `example/ASSESSMENT.md`, not the older baseline.

Recent completed work:

- P0 scan-heavy detection and hybrid guidance
- P1 scholarly first-page cleanup
- P2 initial invoice / form / financial-structure rendering
- P3 decorative image suppression and repeated furniture suppression
- P4 text cleanup / unicode cleanup
- heading normalization for glued numbered headings

So the next plan is about what is **still bad now**.

## Priority order

## Research-backed direction

Recent GitHub research is captured in `example/RESEARCH_NOTES.md`.

Main takeaways:

- keep the converter code-first
- add a dedicated table-specialized pass instead of piling more ad hoc renderer fixes onto all documents
- treat financial reports as a first-class document subtype
- add local OCR preprocessing for scan-heavy PDFs
- improve evaluation with table-specific fixtures and debug artifacts

## P5: Hard financial statement reconstruction

Why now:

- invoices/forms improved a lot
- financial statements are now the biggest structured-doc gap still in active examples

Main failures still visible:

- some rows in `golden__issue-336-conto-economico-bialetti.pdf` still remain smashed
- table output appears in chunks instead of one coherent section
- sub-rows and totals are not always grouped consistently

Work:

1. Improve numeric row splitting for partially recovered rows.
2. Better detect repeated four-column year/value patterns even when labels are long.
3. Merge adjacent financial tables that belong to one logical region.
4. Handle leading section labels like `1.` / `a)` / `17-bis)` without breaking the row parser.
5. Add a table-specialized pass for numeric-heavy regions instead of relying only on generic paragraph rendering.
6. Start detecting financial statements as a document subtype and route them through financial-specific heuristics.

Success bar:

- more of the financial statement becomes proper markdown tables
- fewer fallback raw lines with merged numbers
- totals and sub-items remain aligned

Tests:

- extend ignored financial golden snapshot
- add unit tests for row splitting edge cases
- add expected cell-grid fixtures for a few hard financial regions

## P6: Business header / key-value extraction

Why now:

- invoice body is good now
- header/customer/vendor block still merges fields in awkward ways

Main failures still visible:

- invoice header lines like customer + rep can collapse together
- address blocks are readable but not consistently structured

Work:

1. Improve key-value splitting for inline business headers.
2. Detect address/contact clusters more reliably.
3. Keep line-item table recovery, but improve the metadata area above it.

Success bar:

- invoice sender / recipient / invoice meta reads like a clean field list
- fewer merged contact lines

Tests:

- expand invoice golden checks
- add unit tests around key-value and address heuristics

## P7: Magazine / brochure layout grouping

Why now:

- decorative image spam is much better
- but magazine text still reads like disconnected cover fragments and spread dumps

Main failures still visible:

- cover text still feels fragmented
- brochure/magazine spreads still group some content poorly
- multi-story layouts are not segmented into coherent article blocks

Work:

1. Add layout-mode heuristics for magazine-style pages.
2. Group nearby short headings + body snippets into article clusters.
3. Suppress low-value editorial masthead blocks more aggressively.

Success bar:

- cover pages become easier to skim
- article blurbs group together better
- editorial/legal boilerplate is reduced

Tests:

- extend magazine golden checks
- add targeted renderer tests for cover-style page grouping

## P8: Better scholarly front-page generalization

Why now:

- first-page output is much improved
- but some papers still have dense author blocks or imperfect hierarchy

Main failures still visible:

- `bert`, `clip`, `gpt3`, `resnet`, `physics-hep` still not as clean as `attention`
- affiliations and grouped author lines still vary a lot by paper style

Work:

1. Improve author-cluster grouping across more first-page layouts.
2. Better separate author lines from affiliation blocks.
3. Normalize abstract/title hierarchy across more variants.

Success bar:

- page 1 of major benchmark papers looks consistently human-readable

Tests:

- add more page-1 snapshots for benchmark papers

## P9: OCR / local scan fallback

Why still not first:

- still important
- but currently we at least detect and warn correctly
- bigger immediate quality win may come from improving still-readable docs first

Work:

1. Add optional local OCR preprocessing mode that generates a searchable derived PDF, then reruns the normal extraction path.
2. Keep current warning path when OCR tooling is unavailable.
3. Improve normalization of OCR-ish text when extraction barely works.

Success bar:

- `golden__chinese_scan.pdf` becomes actually usable with a code-driven fallback path

## Suggested next execution order

1. P5 hard financial statement reconstruction
2. add debug artifacts for table/block decisions
3. P6 business header / key-value extraction
4. P7 magazine / brochure grouping
5. P9 OCR / local scan fallback
6. P8 scholarly front-page generalization

## Standing benchmark set

Use these every cycle:

- scholarly:
  - `attention.pdf`
  - `bert.pdf`
  - `clip.pdf`
  - `gpt3.pdf`
  - `resnet.pdf`
  - `survey-llm.pdf`
- business:
  - `golden__issue-336-conto-economico-bialetti.pdf`
  - `PDFUA-Ref-2-02_Invoice.pdf`
  - `PDFUA-Ref-2-10_Form.pdf`
- layout-heavy:
  - `PDFUA-Ref-2-01_Magazine-danish.pdf`
  - `PDFUA-Ref-2-04_Presentation.pdf`
  - `PDFUA-Ref-2-06_Brochure.pdf`
- scan-heavy:
  - `golden__chinese_scan.pdf`
  - `PDFUA-Ref-2-09_Scanned.pdf`
