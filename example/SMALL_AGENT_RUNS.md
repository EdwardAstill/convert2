# Small-Agent Prompt Runs

No actual Haiku model was available in this environment, so I tested the prompts with a small fast agent as a stand-in.

## 1. Paper summary prompt

Input:

- [attention markdown](/home/eastill/projects/convert2/example/markdown/attention/attention.md)

Output:

```text
1. Title
Attention Is All You Need

2. One-sentence summary
The paper introduces the Transformer, a sequence model based only on attention, and reports strong machine translation results with better parallelization and lower training cost than earlier recurrent or convolutional models.

3. Main contribution
A new architecture that removes recurrence and convolutions entirely, using self-attention and encoder-decoder attention as the core mechanism.

4. Key results
The model reports 28.4 BLEU on WMT 2014 English-to-German, described as over 2 BLEU better than prior best results including ensembles. It also reports a new single-model state of the art of 41.8 BLEU on WMT 2014 English-to-French after 3.5 days on eight GPUs. It is also said to work on English constituency parsing.

5. Important caveats from the conversion
The author block is table-like and somewhat noisy, so names/affiliations may be imperfectly rendered. The excerpt shown only includes the abstract, so details beyond the reported metrics are not visible here.
```

Verdict:

- good
- conservative
- exactly the kind of thing a cheap model can do well on decent markdown

## 2. Financial extraction prompt

Input:

- [financial statement markdown](/home/eastill/projects/convert2/example/markdown/golden__issue-336-conto-economico-bialetti/golden__issue-336-conto-economico-bialetti.md)

Output:

```json
{
  "document_type": "income statement excerpt",
  "company": null,
  "customer": null,
  "invoice_number": null,
  "invoice_date": null,
  "totals": {
    "ricavi_delle_vendite_e_delle_prestazioni": "120.062.000, 124.406.000, 115.322.000, 82.776.000",
    "costi_della_produzione": "115.995.000, 116.800.000, 118.293.000, 93.213.000"
  },
  "line_items": [],
  "confidence_notes": [
    "This is not an invoice; it appears to be an Italian financial statement / income statement excerpt.",
    "No company name, customer, invoice number, or invoice date is visible in the provided text.",
    "The markdown shows merged columns with multiple figures on each line, so period-to-value alignment is uncertain.",
    "I only extracted the most obvious top-line totals and left line_items empty to avoid guessing."
  ]
}
```

Verdict:

- good
- prompt correctly prevented fake extraction
- this is exactly why conservative JSON prompts matter on damaged tables

## 3. Form normalization prompt

Input:

- [form markdown](/home/eastill/projects/convert2/example/markdown/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-10_Form/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-10_Form.md)

Output:

```text
# Form title
PDF/UA form example

# Fields
- Text, required: text
- Text, digets: unclear
- Check boxes: check boxes
- Radio buttons: radio buttons
- Drop-down: unclear

# Options
- Check boxes: A, B, C, D
- Radio buttons: yes, no

# Missing / ambiguous structure
- “Text, digets” look OCR-noisy; field type unclear.
- Drop-down has no options shown.
- “From content” may be heading noise or a broken heading.
```

Verdict:

- useful
- not perfect
- still enough to turn rough markdown into a practical field checklist

## Practical advice

Best use for a small agent:

- paper summary
- metadata extraction
- section outline
- form normalization
- conversion-quality critique

Riskier use for a small agent:

- damaged financial tables
- invoices with merged columns
- anything needing exact numeric alignment

Best workflow:

1. Run `cnv` to markdown.
2. Feed markdown to a small cheap model with one of the prompts in [SMALL_AGENT_PROMPTS.md](/home/eastill/projects/convert2/example/SMALL_AGENT_PROMPTS.md).
3. If output says `unclear` or confidence is low, escalate that document to a stronger model or improve the conversion first.
