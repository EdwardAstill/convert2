---
name: summarize-pdf
description: Summarize academic research PDFs into markdown with key images. Takes a PDF path (or glob/directory for batch) and produces a structured markdown summary with extracted figures.
---

# Summarize PDF

Summarize academic research PDFs into structured markdown with key images extracted.

## Arguments

- `<path>` (required) — Path to a PDF file, glob pattern (e.g. `*.pdf`), or directory
- `--output <dir>` (optional) — Output directory. Default: next to the original PDF

## Single PDF Workflow

When given a single PDF file:

1. **Resolve output location.** If `--output` was provided, use that directory. Otherwise, create a folder next to the PDF named after the PDF (without extension). For example, `paper.pdf` produces `paper/paper.md` and `paper/images/`.

2. **Extract text.** Call the `extract_text` MCP tool with the PDF path. If the result has very little text (under 200 characters total), this is likely a scanned PDF — skip to step 5.

3. **Extract images.** Call the `extract_images` MCP tool, saving to a temp directory first.

4. **Summarize and select images.** Read the extracted text and look at the extracted images. Then:
   - Write a structured markdown summary of the paper. Adapt to the paper's own section structure (Abstract, Introduction, Methods, Results, Discussion, etc). Focus on key findings, methodology, and conclusions.
   - Decide which extracted images are important — figures, charts, diagrams that carry meaningful research information. Skip publisher logos, decorative headers, watermarks, and journal formatting elements.
   - For each important image, give it a descriptive filename based on its caption or content (e.g. `fig1-neural-network-architecture.png` not `page3_img2.png`).
   - If you identify important figures in the text that weren't captured by `extract_images` (common for vector-drawn charts), use `render_page` to screenshot those specific pages.
   - Copy selected images to the output `images/` subdirectory with their descriptive filenames.

5. **Scanned PDF fallback.** If text extraction returned very little content:
   - Use `render_page` to screenshot every page
   - Read the page screenshots using your vision capability
   - Produce the summary from the visual content
   - Still extract and rename important figures

6. **Write output.** Write the markdown file with relative image links:
   ```markdown
   ![Figure description](images/fig1-descriptive-name.png)
   ```

7. **Report completion.** Tell the user the summary is ready and where it was saved.

## Large PDF Handling

If the PDF has more than 100 pages:
1. Process in chunks of 20 pages at a time
2. Extract text and images per chunk
3. Summarize each chunk independently
4. After all chunks are processed, produce a final combined summary from the chunk summaries

## Batch Workflow

When given a glob pattern or directory:

1. Resolve all matching `.pdf` files
2. Dispatch one subagent per PDF using the Agent tool. Each subagent should:
   - Be given the full single-PDF workflow instructions above
   - Process its assigned PDF independently
3. After all subagents complete, report which PDFs were summarized and note any failures

## Error Handling

- **Password-protected PDF:** Report the error clearly — "PDF is password-protected, cannot process."
- **Corrupt/unreadable PDF:** Report the filename and error — "Could not read paper.pdf: [error details]"
- **No images found:** Produce the markdown summary without an images folder. This is normal for some papers.
- **Batch partial failure:** Complete all other PDFs and list which ones failed at the end.
