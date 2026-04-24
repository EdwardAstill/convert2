# Tables, Forms, and Financial Documents

This is the highest-value improvement area for `cnv` right now.

## Why these cases matter

Plain prose is relatively forgiving. Tables and forms are not.

When structure is lost:

- totals become ambiguous
- rows become unreadable
- field labels detach from values
- information extraction gets much worse

## The three distinct problem classes

These should not be treated as the same problem.

### 1. Generic tables

Examples:

- comparison tables in papers
- textbook tables
- presentation tables

Main tasks:

- detect the table region
- infer rows and columns
- determine header cells
- preserve cell text

### 2. Forms and key-value documents

Examples:

- forms
- invoices
- application documents
- business letters with metadata blocks

Main tasks:

- detect field labels
- associate values with labels
- preserve checkbox/radio state when extractable
- separate metadata area from body tables

### 3. Financial statements

Examples:

- income statements
- balance sheets
- accounting reports

Main tasks:

- keep multi-column numeric alignment
- preserve section/subsection hierarchy
- distinguish item labels from row markers like `a)` or `17-bis)`
- merge adjacent table fragments into one logical structure

## Why financial statements deserve first-class treatment

The current hard example in `cnv` is not just "a messy table". It has:

- repeated numeric columns
- nested accounting labels
- multi-page continuity
- rows partially smashed by extraction
- section headers that look like body lines

Generic paragraph or list heuristics are the wrong tool for this.

The better approach is:

- detect financial statements as a subtype
- route numeric-heavy regions through a dedicated parser

## A practical table pipeline

The most useful decomposition is:

1. detect candidate table regions
2. infer column guides
3. assign fragments to rows
4. assign cells from row/column intersections
5. classify headers and subheaders
6. render Markdown table or structured key-value output

This mirrors what strong table systems do, even when their internals differ.

## Signals for table detection

Useful table signals:

- repeated right-aligned numeric fragments
- consistent vertical rhythm
- long left label plus several numeric values
- grid lines or rectangles when present
- dense alignment across many lines

Useful form/key-value signals:

- short label followed by delimiter or nearby value
- repeated field patterns
- left label column with sparse right values

## What Markdown can and cannot express well

Markdown is good for:

- rectangular tables
- simple forms rendered as bullet lists
- invoice line items
- flat key-value sections

Markdown is weak for:

- merged cells
- deeply nested financial hierarchies
- complex multi-line headers
- forms with heavy visual semantics

That means the converter sometimes needs to choose between:

- a clean Markdown table
- a field list
- or a fallback text block that preserves meaning better than a broken table

## Recommended implementation work for `cnv`

### Near-term

1. add a table-specialized pass for numeric-heavy regions
2. add a financial-statement subtype detector
3. merge adjacent financial table fragments
4. improve row splitting for partially smashed rows
5. expand invoice header and address parsing

### Mid-term

1. emit debug table artifacts
2. add table-grid fixtures for evaluation
3. support structured intermediate output before Markdown rendering

## Examples in this repo

Useful examples to inspect:

- [Invoice output](../example/markdown/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-02_Invoice/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-02_Invoice.md)
- [Form output](../example/markdown/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-10_Form/golden__pdfua-1-reference-suite-1-1__PDFUA-Ref-2-10_Form.md)
- [Financial statement output](../example/markdown/golden__issue-336-conto-economico-bialetti/golden__issue-336-conto-economico-bialetti.md)
- [Current assessment](../example/ASSESSMENT.md)
- [Current fix plan](../example/FIX_PLAN.md)
