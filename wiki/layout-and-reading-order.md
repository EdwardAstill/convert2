# Layout and Reading Order

Most PDFs fail not because the text is missing, but because the reading order is wrong.

## The real problem

A PDF page is visual, not semantic. The extractor may return fragments in drawing order, not reading order.

That means the converter must infer:

- columns
- paragraphs
- heading proximity
- captions
- sidebars
- page furniture

## Typical layout classes

Different page classes need different handling.

### Single-column text

Usually the easiest case.

Risks:

- headers and footers polluting body text
- footnotes being injected mid-paragraph

### Multi-column papers

Very common in academic PDFs.

Risks:

- left and right columns interleaving
- titles and abstracts getting mixed with author blocks
- figures interrupting column flow

### Slides

Risks:

- repeated slide furniture
- bullet groups rendered as prose
- speaker notes or decorative fragments getting mixed into content

### Magazines and brochures

This is one of the hardest categories.

Risks:

- non-linear reading order
- cover text and hero images dominating layout
- article snippets and captions mixing together
- decorative editorial furniture

### Financial documents

Risks:

- table fragments being mistaken for paragraphs
- right-aligned values losing row structure
- section labels breaking row parsing

## Heuristics that matter

### Column detection

Useful signals:

- repeated vertical gutters
- clusters of blocks with similar widths
- discontinuities in x coordinates

### Block grouping

Useful signals:

- baseline alignment
- inter-line spacing
- x overlap
- font-size consistency
- indentation

### Furniture suppression

Useful signals:

- repeated blocks near page edges
- low-content repeated strings
- repeated logos, slide headers, page numbers

### Heading attachment

Useful signals:

- larger font size
- short line length
- low punctuation density
- proximity to following block

## Recommended implementation direction

`cnv` should continue moving toward:

1. per-page layout classification
2. explicit furniture suppression before final rendering
3. stronger magazine-style page heuristics
4. better attachment of short headings to nearby body blocks
5. debug artifacts for block and reading-order decisions

## Why debug artifacts matter

A markdown diff often hides the real cause of a failure.

For layout debugging, it is useful to emit:

- detected block boxes
- candidate columns
- chosen reading order
- suppressed repeated furniture

These artifacts can make it obvious whether a problem is:

- extraction
- block grouping
- column detection
- or rendering

## Related pages

- [Pipeline Overview](pipeline-overview.md)
- [Tables, Forms, and Financial Documents](tables-forms-and-financials.md)
- [Evaluation and Benchmarks](evaluation-and-benchmarks.md)
