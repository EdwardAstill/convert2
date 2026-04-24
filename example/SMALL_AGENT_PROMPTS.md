# Small-Agent Prompts

These prompts are tuned for a cheap / fast model (the kind of thing you might use in a “Haiku-like” role) on top of `cnv` markdown output.

Rule for all of them:

- pass the markdown, not the PDF
- tell the model to stay conservative
- ask for structured output
- remind it that OCR / layout noise may exist

## 1. Paper summary

Use for research papers.

```text
You are reading markdown converted from a PDF. The conversion may contain layout noise, broken author blocks, or heading mistakes.

Task:
Summarize this paper conservatively.

Return exactly these sections:
1. Title
2. One-sentence summary
3. Main contribution
4. Key results
5. Important caveats from the conversion

Rules:
- Do not invent missing details.
- If the title or authors look noisy, say so.
- Prefer quoting section names only if clearly present.
- Keep the answer under 200 words.

Markdown:
{{MARKDOWN}}
```

## 2. Paper metadata extraction

Use when you want structured fields.

```text
Extract paper metadata from this PDF-to-markdown conversion.

Return JSON with exactly these keys:
- title
- authors
- affiliations
- date
- abstract
- confidence_notes

Rules:
- Use arrays for authors and affiliations.
- If a field is unclear, use null.
- Do not guess missing authors from partial fragments.
- `confidence_notes` should be a short array of extraction problems you noticed.

Markdown:
{{MARKDOWN}}
```

## 3. Section outline

Good for long papers.

```text
This markdown came from a PDF conversion and may contain heading noise.

Task:
Produce a clean section outline.

Return:
- probable title
- ordered list of main sections
- ordered list of subsection headings that look trustworthy
- notes about broken headings or numbering

Rules:
- Ignore obvious metadata lines like arXiv dates unless they are clearly part of a heading.
- If numbering is broken like `1Introduction`, normalize it if obvious.
- Do not include every paragraph; only headings.

Markdown:
{{MARKDOWN}}
```

## 4. Invoice / financial doc extraction

Use for invoices, statements, ledgers.

```text
You are reading markdown converted from a financial PDF. Numbers or columns may be merged together by the conversion.

Task:
Extract the most reliable business fields only.

Return JSON with these keys:
- document_type
- company
- customer
- invoice_number
- invoice_date
- totals
- line_items
- confidence_notes

Rules:
- If columns are smashed together, do not fabricate line-item splits.
- Put only reliable totals into `totals`.
- `line_items` may be an empty array if the structure is too damaged.
- Mention merged-column problems in `confidence_notes`.

Markdown:
{{MARKDOWN}}
```

## 5. Form normalization

Use for forms and questionnaires.

```text
This markdown came from a PDF form conversion.

Task:
Turn it into a clean field list.

Return markdown with:
- Form title
- Fields
- Options
- Missing / ambiguous structure

Rules:
- Represent each field as `Field Name: <type or options>`.
- If a field looks incomplete, mark it as `unclear`.
- Do not invent values that are not present.

Markdown:
{{MARKDOWN}}
```

## 6. Conversion-quality critic

Very useful while improving `cnv`.

```text
You are reviewing markdown produced from a PDF conversion.

Task:
Critique the conversion quality, not the source document.

Return:
1. Score out of 10
2. Biggest structural failures
3. What is still usable
4. What a converter should improve next

Rules:
- Focus on layout, headings, tables, lists, images, and reading order.
- Do not complain about the source PDF itself unless the issue is clearly caused by the source.
- Be concrete.
- Keep it under 180 words.

Markdown:
{{MARKDOWN}}
```

## 7. Retrieval / QA prompt

Use when you want answer-only behavior over converted markdown.

```text
Answer the question using only the markdown below.

Rules:
- If the answer is not clearly present, say `not found in provided markdown`.
- Do not use outside knowledge.
- If the markdown looks corrupted near the relevant part, mention that briefly.
- Quote short phrases only when helpful.

Question:
{{QUESTION}}

Markdown:
{{MARKDOWN}}
```

## 8. Best practice wrapper

If you are calling a small model repeatedly, prepend this once:

```text
The input is markdown converted from a PDF. Common defects:
- broken headings
- author blocks rendered as tables
- merged columns
- image references without surrounding context
- OCR or Unicode noise

Be conservative. Prefer `unclear` over guessing.
```

## Suggested first uses

- `attention.md` -> prompt 1, 2, 3, 6
- `golden__issue-336-conto-economico-bialetti.md` -> prompt 4, 6
- `PDFUA-Ref-2-10_Form.md` -> prompt 5, 6
