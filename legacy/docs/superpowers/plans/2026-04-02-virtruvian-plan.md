# Virtruvian Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Claude Code plugin that summarizes academic research PDFs into markdown with extracted key images.

**Architecture:** A Claude Code plugin containing a local stdio MCP server (TypeScript, `@modelcontextprotocol/sdk`) that exposes PDF processing tools backed by a Python script (PyMuPDF), plus a skill (`/summarize-pdf`) that orchestrates the summarization workflow — calling MCP tools, using Claude's reasoning for summarization and image selection, and writing markdown output.

**Tech Stack:** TypeScript, `@modelcontextprotocol/sdk`, `zod`, Python 3.9+, PyMuPDF (`pymupdf`), Node.js

---

### Task 1: Initialize Project and Plugin Scaffolding

**Files:**
- Create: `package.json`
- Create: `.claude-plugin/plugin.json`
- Create: `.mcp.json`
- Create: `tsconfig.json`
- Create: `src/index.ts`

- [ ] **Step 1: Initialize npm project**

Run: `npm init -y`

Then replace `package.json` contents with:

```json
{
  "name": "virtruvian",
  "version": "0.1.0",
  "description": "Claude Code plugin that summarizes academic research PDFs into markdown with key images",
  "type": "module",
  "main": "build/index.js",
  "bin": "build/index.js",
  "scripts": {
    "build": "tsc",
    "dev": "tsx src/index.ts"
  }
}
```

- [ ] **Step 2: Install TypeScript dependencies**

Run: `npm install @modelcontextprotocol/sdk zod`
Run: `npm install -D typescript @types/node tsx`

- [ ] **Step 3: Create tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "outDir": "build",
    "rootDir": "src",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true
  },
  "include": ["src"]
}
```

- [ ] **Step 4: Create plugin metadata**

`.claude-plugin/plugin.json`:

```json
{
  "name": "virtruvian",
  "version": "0.1.0",
  "description": "Summarize academic research PDFs into markdown with key images. Invoke with /summarize-pdf.",
  "author": {
    "name": "eastill"
  }
}
```

- [ ] **Step 5: Create MCP server config**

`.mcp.json`:

```json
{
  "mcpServers": {
    "virtruvian": {
      "command": "node",
      "args": ["${CLAUDE_PLUGIN_ROOT}/build/index.js"]
    }
  }
}
```

- [ ] **Step 6: Create minimal MCP server entry point**

`src/index.ts`:

```typescript
#!/usr/bin/env node
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

const server = new McpServer(
  { name: "virtruvian", version: "0.1.0" },
  {
    instructions:
      "PDF processing tools for academic papers. Use extract_text to get paper content, extract_images to get embedded images, and render_page to screenshot pages with vector graphics.",
  },
);

const transport = new StdioServerTransport();
await server.connect(transport);
```

- [ ] **Step 7: Build and verify**

Run: `npx tsc`
Expected: No errors, `build/index.js` created.

- [ ] **Step 8: Commit**

```bash
git init
echo "node_modules/\nbuild/\n*.pyc\n__pycache__/\n.venv/" > .gitignore
git add package.json package-lock.json tsconfig.json .gitignore .claude-plugin/plugin.json .mcp.json src/index.ts
git commit -m "feat: initialize virtruvian plugin scaffolding"
```

---

### Task 2: Python PDF Processing Script

**Files:**
- Create: `python/virtruvian_pdf.py`
- Create: `python/requirements.txt`
- Create: `python/setup.sh`

- [ ] **Step 1: Create requirements.txt**

`python/requirements.txt`:

```
pymupdf>=1.24.0
```

- [ ] **Step 2: Create setup script**

`python/setup.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENV_DIR="$SCRIPT_DIR/.venv"

if [ ! -d "$VENV_DIR" ]; then
    python3 -m venv "$VENV_DIR"
fi

"$VENV_DIR/bin/pip" install -q -r "$SCRIPT_DIR/requirements.txt"
echo "$VENV_DIR/bin/python"
```

Run: `chmod +x python/setup.sh`

- [ ] **Step 3: Write the PDF processing script**

`python/virtruvian_pdf.py`:

```python
#!/usr/bin/env python3
"""PDF text and image extraction for Virtruvian.

Usage:
    virtruvian_pdf.py extract_text <pdf_path> [--pages 1,2,3]
    virtruvian_pdf.py extract_images <pdf_path> <output_dir> [--pages 1,2,3]
    virtruvian_pdf.py render_page <pdf_path> <output_path> --page <n> [--dpi 150]

All commands output JSON to stdout.
"""

import argparse
import json
import os
import sys

import pymupdf


def extract_text(pdf_path: str, pages: list[int] | None = None) -> dict:
    doc = pymupdf.open(pdf_path)
    result = {"total_pages": len(doc), "pages": []}

    for i, page in enumerate(doc):
        if pages is not None and (i + 1) not in pages:
            continue
        text = page.get_text("text")
        result["pages"].append({
            "page_number": i + 1,
            "text": text,
        })

    doc.close()
    return result


def extract_images(pdf_path: str, output_dir: str, pages: list[int] | None = None) -> dict:
    doc = pymupdf.open(pdf_path)
    os.makedirs(output_dir, exist_ok=True)
    images = []

    for i, page in enumerate(doc):
        if pages is not None and (i + 1) not in pages:
            continue
        image_list = page.get_images(full=True)

        for img_index, img_info in enumerate(image_list):
            xref = img_info[0]
            base_image = doc.extract_image(xref)
            if base_image is None:
                continue

            ext = base_image["ext"]
            image_bytes = base_image["image"]
            width = base_image["width"]
            height = base_image["height"]

            filename = f"page{i + 1}_img{img_index + 1}.{ext}"
            filepath = os.path.join(output_dir, filename)

            with open(filepath, "wb") as f:
                f.write(image_bytes)

            images.append({
                "page_number": i + 1,
                "image_index": img_index + 1,
                "filename": filename,
                "filepath": filepath,
                "width": width,
                "height": height,
                "ext": ext,
                "size_bytes": len(image_bytes),
            })

    doc.close()
    return {"total_images": len(images), "images": images}


def render_page(pdf_path: str, page_number: int, output_path: str, dpi: int = 150) -> dict:
    doc = pymupdf.open(pdf_path)
    if page_number < 1 or page_number > len(doc):
        doc.close()
        raise ValueError(f"Page {page_number} out of range (1-{len(doc)})")

    page = doc[page_number - 1]
    zoom = dpi / 72
    mat = pymupdf.Matrix(zoom, zoom)
    pix = page.get_pixmap(matrix=mat)

    os.makedirs(os.path.dirname(output_path) or ".", exist_ok=True)
    pix.save(output_path)

    result = {
        "page_number": page_number,
        "filepath": output_path,
        "width": pix.width,
        "height": pix.height,
        "dpi": dpi,
    }

    doc.close()
    return result


def parse_pages(pages_str: str | None) -> list[int] | None:
    if pages_str is None:
        return None
    return [int(p.strip()) for p in pages_str.split(",")]


def main():
    parser = argparse.ArgumentParser(description="Virtruvian PDF processor")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # extract_text
    p_text = subparsers.add_parser("extract_text")
    p_text.add_argument("pdf_path")
    p_text.add_argument("--pages", type=str, default=None)

    # extract_images
    p_images = subparsers.add_parser("extract_images")
    p_images.add_argument("pdf_path")
    p_images.add_argument("output_dir")
    p_images.add_argument("--pages", type=str, default=None)

    # render_page
    p_render = subparsers.add_parser("render_page")
    p_render.add_argument("pdf_path")
    p_render.add_argument("output_path")
    p_render.add_argument("--page", type=int, required=True)
    p_render.add_argument("--dpi", type=int, default=150)

    args = parser.parse_args()

    try:
        if args.command == "extract_text":
            result = extract_text(args.pdf_path, parse_pages(args.pages))
        elif args.command == "extract_images":
            result = extract_images(args.pdf_path, args.output_dir, parse_pages(args.pages))
        elif args.command == "render_page":
            result = render_page(args.pdf_path, args.page, args.output_path, args.dpi)
        else:
            parser.print_help()
            sys.exit(1)

        json.dump(result, sys.stdout, indent=2)
    except Exception as e:
        json.dump({"error": str(e)}, sys.stdout)
        sys.exit(1)


if __name__ == "__main__":
    main()
```

- [ ] **Step 4: Set up Python venv and verify**

Run: `cd python && bash setup.sh`
Expected: Prints path to Python executable in `.venv/`.

Run: `python/.venv/bin/python python/virtruvian_pdf.py --help`
Expected: Shows usage with extract_text, extract_images, render_page subcommands.

- [ ] **Step 5: Commit**

```bash
git add python/
git commit -m "feat: add Python PDF processing script with PyMuPDF"
```

---

### Task 3: MCP Server — Register extract_text Tool

**Files:**
- Create: `src/python-bridge.ts`
- Modify: `src/index.ts`

- [ ] **Step 1: Create the Python bridge module**

This module handles spawning the Python script and parsing JSON output.

`src/python-bridge.ts`:

```typescript
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { access } from "node:fs/promises";

const execFileAsync = promisify(execFile);

const __dirname = dirname(fileURLToPath(import.meta.url));
const PLUGIN_ROOT = resolve(__dirname, "..");
const PYTHON_DIR = resolve(PLUGIN_ROOT, "python");
const VENV_PYTHON = resolve(PYTHON_DIR, ".venv", "bin", "python");
const SCRIPT = resolve(PYTHON_DIR, "virtruvian_pdf.py");

async function ensureVenv(): Promise<void> {
  try {
    await access(VENV_PYTHON);
  } catch {
    const setup = resolve(PYTHON_DIR, "setup.sh");
    await execFileAsync("bash", [setup]);
  }
}

export async function runPythonCommand(
  command: string,
  args: string[],
): Promise<unknown> {
  await ensureVenv();
  const { stdout, stderr } = await execFileAsync(
    VENV_PYTHON,
    [SCRIPT, command, ...args],
    { maxBuffer: 50 * 1024 * 1024 },
  );
  if (stderr) {
    console.error(`Python stderr: ${stderr}`);
  }
  return JSON.parse(stdout);
}
```

- [ ] **Step 2: Register extract_text tool in the MCP server**

Add to `src/index.ts`, after the server instantiation and before the transport connection:

```typescript
import { z } from "zod";
import { runPythonCommand } from "./python-bridge.js";

server.registerTool(
  "extract_text",
  {
    description:
      "Extract text from an academic PDF. Returns structured text with page numbers. Use this to get the paper's content for summarization.",
    inputSchema: {
      pdf_path: z.string().describe("Absolute path to the PDF file"),
      pages: z
        .array(z.number().int().min(1))
        .optional()
        .describe("Specific page numbers to extract. Omit for all pages."),
    },
    annotations: { readOnlyHint: true },
  },
  async ({ pdf_path, pages }) => {
    const args = [pdf_path];
    if (pages && pages.length > 0) {
      args.push("--pages", pages.join(","));
    }
    const result = await runPythonCommand("extract_text", args);
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
    };
  },
);
```

- [ ] **Step 3: Build and verify**

Run: `npx tsc`
Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add src/python-bridge.ts src/index.ts
git commit -m "feat: add extract_text MCP tool with Python bridge"
```

---

### Task 4: MCP Server — Register extract_images Tool

**Files:**
- Modify: `src/index.ts`

- [ ] **Step 1: Add extract_images tool registration**

Add to `src/index.ts`, after the `extract_text` registration:

```typescript
server.registerTool(
  "extract_images",
  {
    description:
      "Extract all embedded images from an academic PDF. Saves images to a temp directory and returns metadata (page number, dimensions, file path). Does NOT capture vector-drawn figures — use render_page for those.",
    inputSchema: {
      pdf_path: z.string().describe("Absolute path to the PDF file"),
      output_dir: z
        .string()
        .describe("Directory to save extracted images to"),
      pages: z
        .array(z.number().int().min(1))
        .optional()
        .describe("Specific page numbers to extract from. Omit for all pages."),
    },
    annotations: { readOnlyHint: false },
  },
  async ({ pdf_path, output_dir, pages }) => {
    const args = [pdf_path, output_dir];
    if (pages && pages.length > 0) {
      args.push("--pages", pages.join(","));
    }
    const result = await runPythonCommand("extract_images", args);
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
    };
  },
);
```

- [ ] **Step 2: Build and verify**

Run: `npx tsc`
Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add src/index.ts
git commit -m "feat: add extract_images MCP tool"
```

---

### Task 5: MCP Server — Register render_page Tool

**Files:**
- Modify: `src/index.ts`

- [ ] **Step 1: Add render_page tool registration**

Add to `src/index.ts`, after the `extract_images` registration:

```typescript
server.registerTool(
  "render_page",
  {
    description:
      "Render a single PDF page as a PNG screenshot. Use this as a fallback when extract_images misses vector-drawn figures, charts, or diagrams. Also useful for scanned PDFs where text extraction fails.",
    inputSchema: {
      pdf_path: z.string().describe("Absolute path to the PDF file"),
      page: z.number().int().min(1).describe("Page number to render (1-indexed)"),
      output_path: z
        .string()
        .describe("File path to save the rendered PNG to"),
      dpi: z
        .number()
        .int()
        .min(72)
        .max(600)
        .default(150)
        .describe("Resolution in DPI. Default 150. Higher = larger file."),
    },
    annotations: { readOnlyHint: false },
  },
  async ({ pdf_path, page, output_path, dpi }) => {
    const args = [pdf_path, output_path, "--page", String(page)];
    if (dpi !== 150) {
      args.push("--dpi", String(dpi));
    }
    const result = await runPythonCommand("render_page", args);
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
    };
  },
);
```

- [ ] **Step 2: Build and verify**

Run: `npx tsc`
Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add src/index.ts
git commit -m "feat: add render_page MCP tool"
```

---

### Task 6: Create the /summarize-pdf Skill

**Files:**
- Create: `skills/summarize-pdf/SKILL.md`

- [ ] **Step 1: Write the skill definition**

`skills/summarize-pdf/SKILL.md`:

````markdown
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
````

- [ ] **Step 2: Verify skill file is well-formed**

Run: `cat skills/summarize-pdf/SKILL.md | head -5`
Expected: Shows the YAML frontmatter with name and description.

- [ ] **Step 3: Commit**

```bash
git add skills/
git commit -m "feat: add /summarize-pdf skill for academic paper summarization"
```

---

### Task 7: End-to-End Testing with MCP Inspector

**Files:** None (manual verification)

- [ ] **Step 1: Build the project**

Run: `npx tsc`
Expected: No errors.

- [ ] **Step 2: Set up Python environment**

Run: `bash python/setup.sh`
Expected: Prints path to venv Python.

- [ ] **Step 3: Test the MCP server with Inspector**

Run: `npx @modelcontextprotocol/inspector --cli --config .mcp.json --server virtruvian --method tools/list`

Expected: JSON listing three tools: `extract_text`, `extract_images`, `render_page` with their schemas.

- [ ] **Step 4: Test extract_text with a sample PDF**

Find or create a sample PDF, then:

Run: `npx @modelcontextprotocol/inspector --cli --config .mcp.json --server virtruvian --method tools/call --tool-name extract_text --tool-arg pdf_path=/path/to/sample.pdf`

Expected: JSON with `total_pages` and array of page text content.

- [ ] **Step 5: Test extract_images with a sample PDF**

Run: `npx @modelcontextprotocol/inspector --cli --config .mcp.json --server virtruvian --method tools/call --tool-name extract_images --tool-arg pdf_path=/path/to/sample.pdf --tool-arg output_dir=/tmp/virtruvian-test-images`

Expected: JSON with `total_images` and array of image metadata. Images saved to `/tmp/virtruvian-test-images/`.

- [ ] **Step 6: Test render_page with a sample PDF**

Run: `npx @modelcontextprotocol/inspector --cli --config .mcp.json --server virtruvian --method tools/call --tool-name render_page --tool-arg pdf_path=/path/to/sample.pdf --tool-arg page=1 --tool-arg output_path=/tmp/virtruvian-test-render.png`

Expected: JSON with page dimensions and filepath. PNG file created at specified path.

- [ ] **Step 7: Commit any fixes**

If any issues were found and fixed during testing:

```bash
git add -A
git commit -m "fix: resolve issues found during end-to-end testing"
```

---

### Task 8: Plugin Installation and Live Test

**Files:** None (manual verification)

- [ ] **Step 1: Install the plugin locally**

Run: `claude plugin add /home/eastill/projects/virtruvian`

Expected: Plugin installed successfully.

- [ ] **Step 2: Verify MCP server is available**

In a new Claude Code session, check that the virtruvian MCP tools appear. Ask Claude to list available MCP tools — `extract_text`, `extract_images`, and `render_page` should be present.

- [ ] **Step 3: Test /summarize-pdf with a real academic paper**

In Claude Code, run:

```
/summarize-pdf /path/to/academic-paper.pdf
```

Expected: Claude processes the PDF, extracts text and images, produces a markdown summary next to the PDF with an images subfolder.

- [ ] **Step 4: Test with --output flag**

```
/summarize-pdf /path/to/academic-paper.pdf --output /tmp/summaries/
```

Expected: Summary written to `/tmp/summaries/academic-paper/academic-paper.md` with images in `/tmp/summaries/academic-paper/images/`.

- [ ] **Step 5: Test batch mode**

```
/summarize-pdf /path/to/papers/*.pdf
```

Expected: Multiple subagents dispatched, each producing a summary. Final report of which succeeded/failed.

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "chore: finalize virtruvian plugin for distribution"
```
