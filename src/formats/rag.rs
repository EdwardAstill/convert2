use std::fs;
use std::path::Path;
use crate::document::types::Document;
use crate::error::{VtvError, VtvResult};
use crate::render::markdown::RenderedDocument;

pub struct RagFormat {
    /// Target chunk size in approximate tokens. Default: 500
    pub target_tokens: usize,
    /// Overlap in approximate tokens between consecutive chunks. Default: 50
    pub overlap_tokens: usize,
}

impl Default for RagFormat {
    fn default() -> Self {
        Self {
            target_tokens: 500,
            overlap_tokens: 50,
        }
    }
}

impl RagFormat {
    pub fn new(target_tokens: usize) -> Self {
        Self {
            target_tokens,
            ..Default::default()
        }
    }

    pub fn write(
        &self,
        rendered: &RenderedDocument,
        doc: &Document,
        output_dir: &Path,
        stem: &str,
    ) -> VtvResult<()> {
        fs::create_dir_all(output_dir).map_err(|e| VtvError::Io {
            path: output_dir.to_path_buf(),
            source: e,
        })?;

        let chunks = self.build_chunks(rendered);
        let total = chunks.len();

        let source_name = doc
            .source_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        for (i, chunk) in chunks.iter().enumerate() {
            let filename = format!("{}_chunk_{:04}.md", stem, i + 1);
            let path = output_dir.join(&filename);

            let frontmatter = format!(
                "---\nsource: {}\nchunk_index: {}\ntotal_chunks: {}\nsection_title: {}\npage_start: {}\npage_end: {}\n---\n\n",
                source_name,
                i + 1,
                total,
                yaml_quote(&chunk.section_title),
                chunk.page_start,
                chunk.page_end,
            );

            let content = format!("{}{}", frontmatter, chunk.content);

            fs::write(&path, &content).map_err(|e| VtvError::Io {
                path: path.clone(),
                source: e,
            })?;
        }

        println!("  wrote {} chunks to {}", total, output_dir.display());
        Ok(())
    }

    #[allow(unused_assignments)]
    fn build_chunks(&self, rendered: &RenderedDocument) -> Vec<Chunk> {
        let mut chunks: Vec<Chunk> = Vec::new();

        if rendered.sections.is_empty() {
            // No sections — chunk the raw markdown directly
            let raw_chunks =
                chunk_text(&rendered.markdown, self.target_tokens, self.overlap_tokens);
            for text in raw_chunks {
                chunks.push(Chunk {
                    section_title: String::from("Document"),
                    page_start: 1,
                    page_end: 1,
                    content: text,
                });
            }
            return chunks;
        }

        let mut current_content = String::new();
        let mut current_title = rendered.sections[0].title.clone();
        let mut current_page_start = rendered.sections[0].page_start;
        let mut current_page_end = rendered.sections[0].page_end;

        for section in &rendered.sections {
            let section_tokens = estimate_tokens(&section.content);

            // If adding this section would overflow and we have content, flush
            if !current_content.is_empty()
                && estimate_tokens(&current_content) + section_tokens > self.target_tokens * 2
            {
                let sub_chunks =
                    chunk_text(&current_content, self.target_tokens, self.overlap_tokens);
                for text in sub_chunks {
                    chunks.push(Chunk {
                        section_title: current_title.clone(),
                        page_start: current_page_start,
                        page_end: current_page_end,
                        content: text,
                    });
                }
                current_content = String::new();
                current_title = section.title.clone();
                current_page_start = section.page_start;
            }

            // Append section heading + content
            let heading = format!("{} {}\n\n", "#".repeat(section.level as usize), section.title);
            current_content.push_str(&heading);
            current_content.push_str(&section.content);
            current_content.push_str("\n\n");
            current_page_end = section.page_end;

            // If current content exceeds target, flush
            if estimate_tokens(&current_content) >= self.target_tokens {
                let sub_chunks =
                    chunk_text(&current_content, self.target_tokens, self.overlap_tokens);
                for text in sub_chunks {
                    chunks.push(Chunk {
                        section_title: current_title.clone(),
                        page_start: current_page_start,
                        page_end: current_page_end,
                        content: text,
                    });
                }
                current_content = String::new();
                current_title = section.title.clone();
                current_page_start = section.page_start;
                current_page_end = section.page_end;
            }
        }

        // Flush remainder
        if !current_content.trim().is_empty() {
            let sub_chunks =
                chunk_text(&current_content, self.target_tokens, self.overlap_tokens);
            for text in sub_chunks {
                chunks.push(Chunk {
                    section_title: current_title.clone(),
                    page_start: current_page_start,
                    page_end: current_page_end,
                    content: text,
                });
            }
        }

        chunks
    }
}

struct Chunk {
    section_title: String,
    page_start: usize,
    page_end: usize,
    content: String,
}

/// Approximate token count: chars / 4 (common heuristic for English text).
fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Split text into chunks of ~target_tokens with overlap.
/// Splits at paragraph boundaries where possible.
fn chunk_text(text: &str, target_tokens: usize, overlap_tokens: usize) -> Vec<String> {
    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for para in &paragraphs {
        let para_tokens = estimate_tokens(para);
        let current_tokens = estimate_tokens(&current);

        if current_tokens + para_tokens > target_tokens && !current.is_empty() {
            chunks.push(current.trim().to_string());

            // Start next chunk with overlap from end of previous
            let overlap_chars = overlap_tokens * 4;
            let prev = chunks.last().unwrap();
            let overlap_start = prev.len().saturating_sub(overlap_chars);
            let overlap_text = &prev[overlap_start..];
            current = overlap_text.to_string();
            current.push_str("\n\n");
        }

        current.push_str(para);
        current.push_str("\n\n");
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() && !text.trim().is_empty() {
        chunks.push(text.trim().to_string());
    }

    chunks
}

/// Quote a string for YAML — wraps in double quotes and escapes internal quotes.
fn yaml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}
