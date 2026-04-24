# Text Extraction

Text extraction is the foundation. If this layer is weak, every later layer becomes guesswork.

## What a PDF actually gives you

A PDF does not contain paragraphs in the way HTML does. It contains drawing instructions:

- draw this glyph
- at this position
- in this font
- at this size

Everything above that level is reconstructed.

For a deeper explanation of object trees, content streams, fonts, encodings, and tagged PDFs, read [the repo's PDF internals guide](../docs/pdf-internals.md).

## Extraction goals

A strong extractor should recover:

- Unicode text where possible
- per-fragment positions
- font sizes
- page geometry
- image blocks
- enough information to reconstruct lines and blocks later

It should avoid making premature assumptions about reading order or semantics.

## Common failure modes

### Missing or broken Unicode mapping

Symptoms:

- blank symbols
- boxes or replacement characters
- garbled math
- missing CJK text

Typical causes:

- bad or missing ToUnicode maps
- subsetted fonts
- Type 3 fonts
- custom encodings

What helps:

- better extraction libraries
- fallback normalization
- OCR for image-based regions

### Over-merged lines

Symptoms:

- two columns become one sentence
- labels glue to values
- headings glue to section numbers

What helps:

- line recovery based on baselines and gaps
- column-aware reading order
- post-extraction normalization for known patterns

### Over-split lines

Symptoms:

- one sentence becomes many short fragments
- author blocks become isolated scraps
- tables lose row integrity

What helps:

- better fragment merging
- document-type-aware grouping

## Practical implementation guidance

### Keep raw geometry

Do not throw away:

- x/y coordinates
- width/height
- page number
- font size

Even if the first renderer does not use all of it, later recovery passes will.

### Preserve uncertainty

When extraction quality is weak, preserve enough raw information that later stages can decide what to do.

Examples:

- mark a block as low-confidence rather than forcing paragraph output
- preserve page-level scan/heaviness signals
- keep numeric alignment signals for later table parsing

### Normalize carefully

Good cleanup:

- remove obvious Unicode junk
- normalize glued headings
- collapse OCR-ish whitespace noise

Bad cleanup:

- deleting symbols that might be meaningful
- flattening structured rows into prose
- normalizing away accounting markers

## What `cnv` should keep improving

1. per-page extraction confidence
2. better handling of mixed text/image PDFs
3. stronger math and symbol recovery where fonts are weak
4. cleaner handoff from extraction to structure recovery

## Related pages

- [Layout and Reading Order](layout-and-reading-order.md)
- [Scans and OCR](scans-and-ocr.md)
- [Markdown Rendering](markdown-rendering.md)
