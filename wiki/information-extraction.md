# Information Extraction

This page is about extracting useful information from PDFs and Markdown in a code-first way.

## Two different tasks

These are related but not the same:

1. convert PDF to Markdown
2. extract structured information from the recovered content

The first task should stabilize the document. The second task should operate on a cleaner representation than raw PDF glyph soup.

## Why Markdown is useful

Markdown gives you:

- headings
- paragraphs
- lists
- tables
- image references

That is often a much better substrate for downstream extraction than raw PDF coordinates.

## Common extraction targets

### Academic papers

Common targets:

- title
- authors
- affiliations
- abstract
- section headings
- figures and tables
- references

### Invoices and business documents

Common targets:

- sender
- recipient
- invoice number
- date
- line items
- totals
- tax

### Forms

Common targets:

- field labels
- field values
- checkbox/radio choices
- missing fields

### Financial statements

Common targets:

- section names
- row labels
- values by year or period
- totals and subtotals

## Recommended extraction strategy

Prefer layered extraction:

1. extract document structure
2. detect document subtype
3. apply subtype-specific field extraction

This is much more reliable than forcing one universal parser over all Markdown.

## Intermediate representations worth keeping

Even if the CLI only outputs Markdown, the internal pipeline benefits from preserving richer objects such as:

- page
- block
- heading
- table
- field
- image
- caption

Those objects make later structured extraction much easier.

## Where `cnv` should improve

1. financial-table structure before field extraction
2. invoice header normalization
3. form-field typing
4. stronger page-1 scholarly extraction
5. preserving confidence signals for ambiguous content

## What not to do

Avoid:

- parsing raw PDF glyph sequences directly when Markdown or structured blocks already exist
- pretending a broken table is safe to extract from
- flattening document-specific structure too early

## Related pages

- [Tables, Forms, and Financial Documents](tables-forms-and-financials.md)
- [Markdown Rendering](markdown-rendering.md)
- [Evaluation and Benchmarks](evaluation-and-benchmarks.md)
