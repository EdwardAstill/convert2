# Pipeline Overview

This page describes the end-to-end shape of a practical PDF-to-Markdown converter, and where `cnv` currently sits.

## The core pipeline

At a high level, conversion should look like this:

1. classify the document and its pages
2. extract text and images
3. recover block structure
4. recover reading order
5. classify blocks into semantic types
6. run specialized recovery passes for hard structures
7. render Markdown
8. evaluate against real examples

In `cnv`, the active path is broadly:

1. open PDF with MuPDF
2. extract text blocks and images
3. reconstruct reading order with XY-Cut++
4. classify blocks into headings, lists, tables, captions, and paragraphs
5. write Markdown plus extracted images

## The most important design rule

Do not treat "PDF to Markdown" as one operation.

Most bad converters fail because they flatten too early. They take low-level text fragments and immediately emit Markdown before they understand:

- what is a heading
- what is body text
- what is a table
- what is decorative furniture
- what is a form field
- what is a scan

The more robust design is:

- first recover structure
- then render

## Recommended internal stages

The following stages should stay distinct internally even if the CLI feels simple.

### 1. Triage

Questions:

- is this text-based, scanned, image-based, or mixed?
- which pages are easy versus hard?
- does this look like a paper, invoice, form, financial statement, slide deck, brochure, or magazine?

Why this matters:

- papers and invoices should not be parsed like brochures
- scan-heavy pages need different handling than clean text pages
- financial statements need a stronger table path than generic text pages

### 2. Extraction

Output should include:

- text fragments
- coordinates
- font sizes
- block bounds
- image blocks
- page dimensions

This stage should avoid making strong semantic decisions.

### 3. Structural recovery

Build an intermediate representation with objects such as:

- heading
- paragraph
- list
- table
- caption
- image
- form field
- page furniture

This is the stage where most future quality gains should land.

### 4. Specialized recovery passes

Generic paragraph logic is not enough for all pages.

Important specialized passes:

- numeric-heavy table pass
- form-field pass
- invoice/business-document pass
- scholarly front-page pass
- scan/OCR pass

### 5. Rendering

Markdown generation should happen after structure is stable.

The renderer should be simple compared to the recovery stages. If rendering logic grows into a maze of document-specific heuristics, that usually means structure recovery is underpowered.

## Current architecture direction for `cnv`

Based on current failures and research, the next good architecture moves are:

1. stronger per-page triage
2. table-specialized parsing for numeric-heavy regions
3. financial-statement document subtype
4. optional local OCR preprocessing
5. richer intermediate representation before Markdown rendering
6. debug artifacts for layout and table decisions

## Practical anti-patterns

Avoid these:

- one-pass "extract and immediately print Markdown"
- making all document classes share the same heuristics
- relying only on body-text grouping for tables
- mixing scan handling into normal text extraction without a clear route
- putting too much structure logic directly in the Markdown renderer

## Where to go next

- [Text Extraction](text-extraction.md)
- [Layout and Reading Order](layout-and-reading-order.md)
- [Tables, Forms, and Financial Documents](tables-forms-and-financials.md)
