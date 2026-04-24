# Virtruvian — Design Spec

A Claude Code plugin that summarizes academic research PDFs into markdown with extracted key images, so users don't need to deal with large PDF files directly.

**Scope:** Academic papers and research studies specifically.

## Architecture

Two components packaged as a single Claude Code plugin:

### 1. `virtruvian-mcp` — MCP Server

A TypeScript MCP server (using `@modelcontextprotocol/sdk`) that exposes PDF processing tools. It delegates heavy PDF work to a bundled Python script (`virtruvian_pdf.py`) using PyMuPDF (`pymupdf`). The MCP server spawns the Python script as a child process and parses JSON output.

**Tools exposed:**

- **`extract_text(pdf_path: string, pages?: number[])`** — Extracts text from the PDF with page numbers. Optionally limited to specific pages. Returns structured text as JSON.

- **`extract_images(pdf_path: string, pages?: number[])`** — Extracts all embedded images from the PDF. Saves them to a temp directory. Returns metadata per image: page number, dimensions, position on page, and file path.

- **`render_page(pdf_path: string, page: number, dpi?: number)`** — Renders a full page as a PNG. Default DPI: 150. Used as a fallback for vector-based figures/charts that aren't captured as embedded images.

### 2. `summarize-pdf` — Skill (Slash Command)

A Claude Code skill invoked as `/summarize-pdf`. Orchestrates the full workflow: argument parsing, MCP tool calls, Claude-driven summarization and image selection, and output writing.

## Skill Invocation

```
/summarize-pdf paper.pdf
/summarize-pdf paper.pdf --output ./summaries/
/summarize-pdf *.pdf
/summarize-pdf ./papers/
```

**Arguments:**
- `<path>` (required) — Path to a PDF file, a glob pattern, or a directory
- `--output <dir>` (optional) — Output directory. Default: next to the original PDF

## Single PDF Workflow

1. Parse arguments — resolve PDF path and output location
2. Call `extract_text` to get the full paper text
3. Call `extract_images` to get all embedded images
4. Claude reads the text and images, then:
   - Generates a structured markdown summary, adapting to the paper's own section structure (e.g. Abstract, Methods, Results, Discussion)
   - Selects which images are important — figures, charts, key diagrams — skipping logos, publisher marks, and decorative elements
   - If an important figure is vector-based and wasn't captured by `extract_images`, falls back to `render_page` to screenshot that page
5. Writes output to the determined location

## Batch Workflow

When given a glob pattern or directory:
1. Resolve all matching PDF files
2. Dispatch one subagent per PDF, each running the single-PDF workflow in parallel
3. Print a summary when all complete, noting any failures

## Output Structure

Default (next to PDF):
```
paper.pdf
paper/
  paper.md
  images/
    fig1-results-chart.png
    fig2-methodology-diagram.png
```

With `--output ./summaries/`:
```
summaries/
  paper/
    paper.md
    images/
      fig1-results-chart.png
      fig2-methodology-diagram.png
```

The markdown file uses relative links to the images folder:
```markdown
![Results chart](images/fig1-results-chart.png)
```

## Image Selection

Claude decides which images are important based on context. The goal is to extract images that carry meaningful information about the research — figures, charts, diagrams, tables rendered as images. Skip decorative elements, publisher logos, headers, and watermarks.

Image filenames are descriptive, derived from the figure caption or content (e.g. `fig1-results-chart.png` rather than `image_001.png`).

## Error Handling

- **Scanned/image-only PDFs:** If `extract_text` returns little or no text, fall back to `render_page` for each page and let Claude read the pages via vision
- **Password-protected PDFs:** Fail with a clear error message — no password prompting
- **Corrupt/unreadable PDFs:** Fail gracefully with the filename and error reason
- **No images found:** Produce the markdown summary without an images folder
- **Very large PDFs (100+ pages):** Process in page-range chunks (e.g. 20 pages at a time), summarize each chunk independently, then produce a final combined summary from the chunk summaries
- **Batch failures:** Complete all other PDFs and report which ones failed at the end

## Technology Stack

- **MCP Server:** TypeScript, `@modelcontextprotocol/sdk`
- **PDF Processing:** Python 3.9+, PyMuPDF (`pymupdf`)
- **Plugin Runtime:** Node.js

## Plugin Packaging

- Single plugin repo containing both the MCP server and the skill
- Setup automatically checks for Python 3.9+ and creates a local venv with PyMuPDF installed
- Plugin name: `virtruvian`
