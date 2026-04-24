# Example Assessment

This assessment reflects the **current** `cnv` output in `example/markdown/` after the recent code-first improvements:

- scan-heavy detection and hybrid guidance
- scholarly first-page cleanup
- text cleanup / unicode cleanup
- invoice / form / key-value rendering
- decorative image suppression
- heading normalization (`1Introduction` -> `1 Introduction`)

Assessment method:

- regenerated `example/markdown/` from the current binary
- spot-checked output against source PDFs and `pdftotext`
- scored for usefulness as markdown, not pixel-perfect visual fidelity

Scoring guide:

- `9-10`: very usable, minor cleanup only
- `7-8`: good, but still some structural noise
- `5-6`: mixed, useful with caution
- `0-4`: poor, major structure or text loss

## Per-file scores

| File | Score | Current state | Biggest remaining problems |
| --- | ---: | --- | --- |
| `attention.pdf` | 9/10 | First page is much better: title promoted correctly, author block no longer a table, abstract clean, section heading glue fixed. | Permission note still sits in page 1 body; author grouping can still be cleaner. |
| `bert.pdf` | 8/10 | Strong abstract/body extraction. | Author block still flatter than ideal; title hierarchy still a bit rough. |
| `clip.pdf` | 8/10 | Good abstract/body output, generally usable. | Front-page author block still dense. |
| `golden__1901.03003.pdf` | 7/10 | Better than before; still readable. | Page-1 image noise is reduced but not fully solved. |
| `golden__2408.02509v1.pdf` | 9/10 | Strong scholarly extraction. | Minor polish only. |
| `golden__chinese_scan.pdf` | 3/10 | Still poor locally, but now clearly flagged as scan-heavy with hybrid guidance. | Needs OCR or hybrid path to become genuinely useful. |
| `golden__issue-336-conto-economico-bialetti.pdf` | 7/10 | Big improvement: many rows now become markdown tables with recovered numeric columns. | Some rows still remain smashed and split inconsistently across sections. |
| `golden__lorem.pdf` | 10/10 | Clean and accurate. | None worth chasing. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-01_Magazine-danish.pdf` | 6/10 | Much less image spam on page 1; more text-first now. | Magazine spread reading order and content grouping still rough. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-02_Invoice.pdf` | 9/10 | Real win: line items render as a markdown table and total is obvious. | Header/customer block still merged in a few places. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-03_AcademicAbstract.pdf` | 8/10 | Good extraction. | Author/affiliation layout can still be improved. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-04_Presentation.pdf` | 8/10 | Repeated slide furniture/header junk is now much better suppressed. | Slide semantics still read more like prose dump than structured slides. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-05_BookChapter-german.pdf` | 8/10 | Cleaner than before, readable long-form output. | Some residual encoding / typography cleanup still possible. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-06_Brochure.pdf` | 7/10 | Decorative clutter reduced somewhat. | Brochure layout still not grouped cleanly enough. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-08_BookChapter.pdf` | 9/10 | Strong long-form output. | Minor hierarchy polish only. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-09_Scanned.pdf` | 8/10 | Still surprisingly usable. | OCR-like normalization and heading cleanup can improve it further. |
| `golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-10_Form.pdf` | 8/10 | Big improvement: form now renders as labeled entries and options. | Field typing is still heuristic and shallow. |
| `gpt3.pdf` | 8/10 | Strong text output. | Author block still dense on page 1. |
| `math-number-theory.pdf` | 8/10 | Good extraction, math-heavy abstract still readable. | Some symbol / footnote polish still needed. |
| `physics-hep.pdf` | 7/10 | Better than baseline, still readable. | Front-page author/affiliation layout and media placement can improve. |
| `resnet.pdf` | 8/10 | Improved scholarly-page behavior helps. | Author/title/abstract structure still not as clean as best cases. |
| `survey-llm.pdf` | 9/10 | Very usable large-paper output. | Minor front-page polish only. |

## Current take

The tool is now:

- **good** on text-heavy academic PDFs
- **good** on invoices and simple forms
- **decent** on scanned/tagged textbooks when text is extractable
- **improved but still mixed** on financial statements
- **still weakest** on magazine/brochure/spread layouts and image-only scans

## Biggest remaining gaps

1. OCR-less scans still fail badly.
2. Financial statement reconstruction is only partial.
3. Magazine / brochure layout grouping still needs real work.
4. Scholarly first-page handling is much better, but not fully solved across all papers.
5. Business document header/customer key-value splitting can still improve.
