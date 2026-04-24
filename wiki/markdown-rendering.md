# Markdown Rendering

Rendering is the final stage, not the main stage.

If Markdown output is bad, the real problem is often earlier:

- extraction
- layout recovery
- table reconstruction
- scan handling

## Rendering goals

Markdown output should be:

- readable
- stable
- diff-friendly
- useful for downstream tools

That means preserving meaning matters more than preserving visual layout.

## Preferred output shapes

### Headings

Use headings when structure is genuinely present.

Good:

- paper titles
- section headings
- chapter headings

Bad:

- promoting random bold fragments into headings

### Paragraphs

Paragraph output should preserve sentence flow without column bleed or line-chop noise.

### Lists

Use list output when bullets or enumerations are actually present.

### Tables

Use GFM tables when the structure is reasonably rectangular.

### Images

Use standard image references:

```md
![image](images/page3_img1.png)
```

### Forms

Simple forms usually read better as labeled bullet items than as broken pseudo-tables.

## Renderer design rules

### Keep renderer logic smaller than structure logic

A renderer should mostly format already-known structure.

If the renderer is responsible for:

- discovering tables
- repairing reading order
- suppressing page furniture
- inferring document subtype

then the pipeline is leaking too much uncertainty into the last stage.

### Prefer useful Markdown over visually faithful Markdown

Examples:

- a clean invoice table is better than preserving every horizontal line
- a field list is better than a malformed table
- suppressing repeated headers is better than preserving every page artifact

### Normalize known nuisances

Reasonable examples:

- glued headings like `1Introduction`
- repeated decorative image spam
- trivial Unicode cleanup

## What downstream consumers want

Markdown consumers often care about:

- stable headings
- coherent paragraphs
- tables that parse
- predictable image paths
- minimal noise

This matters for:

- search and indexing
- embeddings and chunking
- human reading
- structured extraction from Markdown

## Recommended future direction

1. keep moving structure recovery upstream
2. keep renderer deterministic
3. add confidence-aware fallbacks for hard blocks
4. avoid adding document-class-specific hacks only in the renderer when a structural pass would be cleaner

## Related pages

- [Pipeline Overview](pipeline-overview.md)
- [Text Extraction](text-extraction.md)
- [Information Extraction](information-extraction.md)
