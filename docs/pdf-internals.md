# PDF internals — the parts that matter to `cnv`

This is a primer on how PDF files are actually structured, aimed at contributors who need to understand why `cnv`'s PDF-to-markdown pipeline behaves the way it does — why some things come through cleanly, why others are silently lost, and where the interesting hooks are for future improvements.

It is opinionated: it only covers the PDF features that `cnv` touches, avoids, or is blocked by. It is not a replacement for the 1,000-page ISO 32000-2 specification. For the full formal definition, read that. For the working mental model, read this.

## The 30-second mental model

A PDF file is a tree of versioned, reference-counted **objects**. At the root is a `Catalog`. The catalog points to a `Pages` node, which points to an ordered list of `Page` objects. Each `Page` has a **content stream** — a program in a small stack-based graphics language — that, when interpreted, paints the page. Text is drawn by calling `Tj` / `TJ` operators that place glyphs from a specified font at specified positions. Everything else — fonts, images, forms, overlays — is a dictionary hanging off a page or the catalog.

That is the entire architecture. Fifteen words: *tree of objects, each page is a program, text is drawn as glyphs*. Everything the `cnv` codebase worries about flows from that shape.

## The object tree

Concretely, a PDF is a sequence of versioned objects:

```
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj

2 0 obj
<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>
endobj

3 0 obj
<< /Type /Page
   /Parent 2 0 R
   /Contents 5 0 R
   /Resources << /Font << /F1 7 0 R >> >>
   /MediaBox [0 0 612 792]
>>
endobj
```

`1 0 R` is a reference to object 1, generation 0. The catalog, pages node, and page objects form a tree. `/Contents` on a page points to the content-stream object. `/Resources` names the fonts and images the content stream will reference by short alias (`F1`, `Im1`, …).

`cnv` never walks this tree directly. mupdf does it all for us. What matters is that the model has named resources per page — so when mupdf tells us "this glyph was drawn in font F1", we know F1 is a specific, embedded font object whose attributes (family, subtype, encoding, ToUnicode map) are recorded in the PDF. Most of what `cnv` can *eventually* do — font-name-aware heading detection, struct-tree awareness, correct ligature handling — depends on reading those attributes, and the current mupdf Rust wrapper does not expose them.

## The content stream — a tiny graphics VM

Each page's `/Contents` is a byte stream of **postfix operators**. A small operand stack, a small graphics-state stack, and maybe thirty operators you actually care about. Here is a minimal page:

```
BT                      % begin text object
  /F1 12 Tf             % select font F1 at 12 points
  72 720 Td             % move text cursor to (72, 720) in page coords
  (Hello, world.) Tj    % show the literal string
ET                      % end text object
```

Key text operators:

| op | meaning |
|---|---|
| `Tf` | select font and size |
| `Td` / `TD` / `Tm` | move or set the text matrix (cursor position) |
| `Tj` | show a string of glyphs |
| `TJ` | show an array of strings with explicit inter-glyph spacing (common in justified text) |
| `'` / `"` | newline-and-show variants |

Key graphics operators relevant to text extraction:

| op | meaning |
|---|---|
| `cm` | concatenate to current transformation matrix (rotation, scale) — rotated pages are laid out via `cm` |
| `Do` | invoke an XObject — this is how embedded images and vector-graphic symbols get pulled in |
| `m l c re h S f B W n` | path construction / fill / stroke / clip — the vector-graphics operators |

When mupdf extracts text, it interprets the content stream, tracks the transformation matrix and text matrix, and produces a list of glyphs each with an origin (x, y), a size, and a character code. `cnv` consumes that list.

The crucial thing to understand is that **there is no concept of "paragraphs" or "lines" in the content stream itself.** A PDF content stream knows only about glyphs at positions. Anything higher-level — word boundaries, line breaks, reading order, columns, tables — is *reconstructed* by the extractor from glyph positions. This is why PDF-to-text is hard and why projects like `cnv` have layers of heuristics (XY-Cut++, font-size classification, etc.) sitting on top of the raw glyph stream.

## Coordinate systems — two gotchas

1. **PDF page space has its origin at the bottom-left, y growing upward.** That is the classical mathematical convention but the opposite of every screen-based coordinate system you have ever touched. It is also the convention the OpenDataLoader Java implementation of XY-Cut++ uses.
2. **`cnv` uses top-left origin with y growing downward**, because that is what the mupdf Rust wrapper returns from `page.bounds()` and `block.bounds()`. So somewhere between "PDF native" and "what `cnv` sees", a flip happens.

The flip matters for two specific modules:

- `src/layout/xycut.rs` — the ported Java algorithm uses `topY > bottomY` comparisons; the Rust port inverts them to `y0 < y1`. The module doc-comment has the translation table.
- Any future direct integration with tagged-PDF tooling (pdfium-render, PDFBox) will return native PDF coordinates; conversions will have to flip.

Rotation (portrait vs landscape) is a property of the page's `/Rotate` key, applied via the initial CTM. mupdf normalises it so extracted glyphs come back in a canonical orientation. `cnv` does not handle rotated-text annotations itself — it relies on mupdf to unwind them.

## Fonts, encodings, and why text sometimes vanishes

This is the single most operationally important section of the document. It explains why `cnv` occasionally returns blank text for an equation, or why one PDF's math is perfect and the next one's has `□` in place of `∫`.

PDF supports four font types:

- **Type 1** — classic PostScript fonts. Small, old, common in academic PDFs generated by older LaTeX.
- **TrueType** — modern outline fonts. Most common today.
- **Type 3** — fonts whose glyphs are themselves small PDF content streams. Rare but seen in old TeX output and some scientific typesetters. These are effectively "draw this glyph by executing these vector-graphics operators".
- **Type 0** (also called **CID**) — composite fonts used for large character sets (CJK, Devanagari, some math fonts). Glyphs are identified by CID (character ID), not by Unicode codepoint.

For every font, the PDF optionally carries a **ToUnicode CMap** — a mapping from the internal glyph code (1 byte or 2 bytes or CID) to the actual Unicode codepoint the glyph represents. Text extraction is *only* meaningful when this map is present and accurate.

A ToUnicode CMap looks like:

```
/CIDInit /ProcSet findresource begin
12 dict begin
beginbfchar
  <0001> <0041>         % glyph 1 is Unicode U+0041 'A'
  <0002> <0042>         % glyph 2 is 'B'
  <0003> <0040>         % glyph 3 is '@'
endbfchar
```

**If the CMap is missing for a glyph**, the text extractor has to guess — or, more often, give up. In mupdf this manifests as `TextChar::char() -> None`. `cnv`'s extractor filters these out (`src/pdf/extractor.rs:203` and adjacent), meaning glyphs without Unicode mappings are **silently dropped**. No warning, no placeholder — just a hole in the output.

When does this happen?

- **Subsetted fonts with stripped ToUnicode.** LaTeX via `pdftex` routinely subsets fonts (ships only the glyphs actually used) and historically has done a mediocre job of emitting ToUnicode for math symbols. Symbols like `∫`, `∑`, `⊂`, `ℕ` can vanish.
- **Type 3 fonts.** Since the glyph is a vector-drawn path, there is nothing intrinsically tying it to a Unicode codepoint. The PDF might or might not supply a mapping.
- **Custom encodings.** Some older PDFs use `/Differences` arrays in their encoding dict to remap glyph positions — if this remapping is not reflected in the ToUnicode, text comes out as garbage.

What would fix it? **An OCR pass over the rendered pixels** — which is exactly what Docling's hybrid mode does — or a dedicated math-OCR pass on equation regions (pix2tex, texify, UniMERNet). This is the plan for Phase 2b and beyond.

### Ligatures

Many fonts ship glyph forms for common pairs like `fi`, `fl`, `ffi`. When extracted, these should ideally round-trip back to the two or three component Unicode characters. mupdf decomposes ligatures by default. `cnv` does not pass the `PRESERVE_LIGATURES` flag, so we get the decomposed form. Good — search indexes, RAG embeddings, and markdown renderers all expect `fi` not `ﬁ`.

### Font name exposure

mupdf 0.6.0's Rust wrapper does not expose per-glyph font names. `PdfExtractor::dominant_font_name` at `src/pdf/extractor.rs:318` returns the literal string `"unknown"`. This is why `src/layout/classifier.rs` is size-only — it cannot distinguish a 10-pt heading from a 10-pt body paragraph, even when the PDF marks the former as bold Helvetica and the latter as regular Minion. Adding `pdfium-render` as a second-opinion reader for font metadata is Phase 3 of the plan.

## The structure tree — what "tagged PDF" means

PDF 1.4 introduced a parallel data structure called the **structure tree**. It lives at `/Catalog → /StructTreeRoot` and mirrors the document's logical structure — chapters, sections, paragraphs, figures, tables — entirely independently of where those items are physically drawn on the page. A well-tagged PDF reads like a DOM tree:

```
StructTreeRoot
├── H1 ("Abstract")
│   └── P (alt: "We propose a Transformer...")
├── H1 ("1 Introduction")
│   ├── P
│   ├── P
│   └── Figure (alt: "Figure 1. The Transformer architecture")
└── H1 ("2 Background")
    └── Table (summary: "Benchmark results")
        └── TR
            ├── TH (scope: col)
            ├── TH
            └── TD
```

Roles are drawn from a standard set: `H1`..`H6`, `P`, `L`, `LI`, `Figure`, `Table`, `TR`, `TD`, `TH`, `Span`, `Reference`, `Note`, etc. Anything custom can be registered in `/RoleMap`.

Each `StructElement` can carry `/Alt` text (an accessibility description), `/ActualText` (the canonical text for glyphs whose visible form is decorative), `/Lang` (language code), and references to the pages and content-stream markers where it is drawn (`/K` kids).

When present, the structure tree is *the authoritative source of truth* for what the document means. No heuristics needed — no font-size thresholds, no reading-order recovery, no caption detection. You just walk the tree.

### Who produces tagged PDFs?

- Word, via `Save As PDF/A`, produces solid tags.
- Adobe Acrobat autotags reasonably well.
- LaTeX via `\usepackage{accessibility}` or `tagpdf` can produce tags, but most academic PDFs do not use it.
- OpenDataLoader both **reads and writes** tagged PDFs (their `--format tagged-pdf` output emits `/StructTreeRoot`).

Most PDFs in the wild — especially academic arXiv PDFs — are **not tagged**. Heuristics remain necessary. But when a tagged PDF is encountered, defaulting to font-size classification is leaving free quality on the table.

### Why `cnv` does not read the struct tree today

mupdf's Rust wrapper does not surface `/StructTreeRoot`. To access it from Rust we need a different library. The research done for Phase 3 concluded that `pdfium-render` is the right path: it wraps Chromium's PDFium, exposes `FPDF_StructTree_*` bindings, and can be added behind a Cargo feature flag so the default build does not require `libpdfium` at runtime.

## Images — how they actually sit in a PDF

Images in PDFs are **XObjects** of subtype `/Image`. A single image looks like:

```
10 0 obj
<< /Type /XObject /Subtype /Image
   /Width 800 /Height 600
   /ColorSpace /DeviceRGB /BitsPerComponent 8
   /Filter /DCTDecode
   /Length 123456
>>
stream
...raw JPEG bytes...
endstream
```

The `/Filter` determines the encoding:

| Filter | What it is |
|---|---|
| `DCTDecode` | JPEG. Most common for photos. |
| `FlateDecode` | zlib-compressed raw pixels. Common for diagrams, screenshots. |
| `LZWDecode` | older TIFF-style. Rare in modern PDFs. |
| `CCITTFaxDecode` | fax compression (1-bit). Scanned black-and-white pages. |
| `JBIG2Decode` | binary-image compression. Used by heavy scan-to-PDF tools. |
| `JPXDecode` | JPEG 2000. Rare but seen in high-quality scanned docs. |

An image is invoked from a page's content stream by `/Im1 Do`. Where `Im1` is an alias in `/Resources /XObject` pointing at the XObject. The position and scaling come from the `cm` operators issued before the `Do`.

### How `cnv` extracts images today

mupdf's `TextBlock::image()` returns an `Option<Image>` for image-type blocks. `cnv`'s extractor (`src/pdf/extractor.rs`, Image arm in `extract_page`) calls `image.to_pixmap()` followed by `pixmap.write_to(bytes, ImageFormat::PNG)` to get PNG-encoded bytes, regardless of the original filter. That means:

- Original JPEGs are transcoded to PNG. Lossless re-encoding of a lossy source — slightly wasteful, fine in practice.
- 1-bit fax-compressed images become 24-bit PNG. Bigger files, readable output.
- JPEG 2000 and JBIG2, if mupdf's pixmap converter can decode them, round-trip to PNG. Otherwise we skip silently.

The PNG is written to `<output>/images/page{N}_img{M}.png`, and a `Block { kind: BlockKind::Image { path: "images/..." } }` is inserted into the page in Y-position order. The resulting markdown carries `![image](images/page3_img1.png)` links.

### What `cnv` does not extract

- Vector graphics drawn via `m l c S f` — ResNet's architecture diagrams, arXiv's TikZ figures, most flow charts. mupdf classifies these as `TextBlockType::Vector`, and our extractor drops `Vector` blocks explicitly.
- Form fields (AcroForm). These are interactive UI elements laid over the page, drawn by the viewer not the content stream.
- Annotations — highlights, comments, stamps — live in `/Annots` on the page dict. We do not read them.
- Bookmarks / outlines live in `/Outlines` under the catalog. Not read.

## The full list of what `cnv` touches and what it skips

### Touched

- Content-stream text glyphs, with positions and font sizes.
- Raster image XObjects, via mupdf's `Image::to_pixmap`.
- Document-level metadata: `/Title`, `/Author`, `/Subject` from the info dictionary.

### Partially touched

- Fonts: size only. Family, weight, style, encoding are read by mupdf internally but not surfaced to our extractor.

### Skipped

- Structure tree (`/StructTreeRoot`).
- Vector graphics.
- Annotations, form fields.
- Outlines, OCG (optional content groups / layers), JavaScript, embedded files.
- Encryption — password-protected PDFs error out; no attempt to prompt for or derive the password.
- Digital signatures (we do not verify them; text beneath signed sections is extracted normally).

### Fundamentally problematic

- Glyphs whose font has no ToUnicode mapping — **silently dropped**. Biggest quality risk for math-heavy PDFs. Mitigations: hybrid Docling path (Phase 2) runs OCR; Phase 3 pdfium-render metadata could help detect when a block has many such glyphs and flag the page for triage; Phase 4+ could integrate pix2tex / UniMERNet for per-crop math OCR.
- Mathematical equations as a whole. A PDF equation is rendered as a small block of glyph positions, sometimes drawn from multiple fonts, sometimes with symbols drawn as vector paths in Type 3 fonts. There is no `(equation)` block type in PDF. Reconstructing LaTeX from that requires vision-model OCR — no heuristic approach gets above about 60 % BLEU.

## Further reading

- **ISO 32000-2** — the formal specification. Pay-walled but available as a draft via Adobe (`PDF32000_2008.pdf` on adobe.com, which is the older ISO 32000-1 but almost all of it is still correct).
- **PDF Explained, O'Reilly** by John Whitington — readable 100-page introduction.
- The **mupdf source tree** at `https://git.ghostscript.com/mupdf.git`. Especially `source/pdf/pdf-font.c`, `source/pdf/pdf-cmap.c`, and `source/pdf/pdf-structure.c`.
- **PDF.js** (`https://github.com/mozilla/pdf.js`) — pure-JS renderer; its `core/` directory is a readable tour of the PDF internals.
- The **OpenDataLoader benchmark paper** describing XY-Cut++ (arXiv:2504.10258) for the reading-order side of text extraction.
