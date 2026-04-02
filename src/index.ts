#!/usr/bin/env node
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { runPythonCommand } from "./python-bridge.js";

const server = new McpServer(
  { name: "virtruvian", version: "0.1.0" },
  {
    instructions:
      "PDF processing tools for academic papers. Use extract_text to get paper content, extract_images to get embedded images, and render_page to screenshot pages with vector graphics.",
  },
);

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

const transport = new StdioServerTransport();
await server.connect(transport);
