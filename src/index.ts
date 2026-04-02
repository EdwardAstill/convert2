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
