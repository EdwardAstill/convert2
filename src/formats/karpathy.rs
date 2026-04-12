use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use crate::document::types::Document;
use crate::error::{VtvError, VtvResult};
use crate::render::markdown::RenderedDocument;

pub struct KarpathyFormat;

impl KarpathyFormat {
    pub fn write(
        rendered: &RenderedDocument,
        doc: &Document,
        output_dir: &Path,
        stem: &str,
    ) -> VtvResult<()> {
        fs::create_dir_all(output_dir).map_err(|e| VtvError::Io {
            path: output_dir.to_path_buf(),
            source: e,
        })?;

        if rendered.sections.is_empty() {
            // Fall back to single file
            let md_path = output_dir.join(format!("{}.md", stem));
            fs::write(&md_path, &rendered.markdown).map_err(|e| VtvError::Io {
                path: md_path,
                source: e,
            })?;
            return Ok(());
        }

        // Build set of all section titles for wikilink matching
        let title_set: HashSet<String> = rendered.sections
            .iter()
            .map(|s| s.title.clone())
            .collect();

        // Write each section as its own file
        for section in &rendered.sections {
            let filename = format!("{}.md", slugify(&section.title));
            let path = output_dir.join(&filename);

            let heading = format!("{} {}\n\n", "#".repeat(section.level as usize), section.title);
            let body = inject_wikilinks(&section.content, &title_set, &section.title);
            let content = format!("{}{}", heading, body);

            fs::write(&path, &content).map_err(|e| VtvError::Io {
                path: path.clone(),
                source: e,
            })?;
        }

        // Write index.md
        let title = doc.metadata.title.as_deref().unwrap_or(stem);
        let mut index = format!("# {}\n\n## Sections\n\n", title);
        for section in &rendered.sections {
            index.push_str(&format!("- [[{}]]\n", section.title));
        }

        let index_path = output_dir.join("index.md");
        fs::write(&index_path, &index).map_err(|e| VtvError::Io {
            path: index_path.clone(),
            source: e,
        })?;

        println!("  wrote {} section files + index.md to {}",
            rendered.sections.len(), output_dir.display());
        Ok(())
    }
}

/// Convert a section title to a safe filename slug.
/// "Related Work" → "related_work"
pub fn slugify(title: &str) -> String {
    title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

/// Replace occurrences of section titles (case-insensitive) in content with [[WikiLinks]].
/// Skips the current section's own title to avoid self-links.
/// Uses a simple greedy left-to-right scan — no regex, no NLP.
fn inject_wikilinks(content: &str, title_set: &HashSet<String>, current_title: &str) -> String {
    // Sort titles longest-first to match greedily (avoids "Introduction" matching inside "Introduction to X")
    let mut titles: Vec<&String> = title_set
        .iter()
        .filter(|t| t.as_str() != current_title)
        .collect();
    titles.sort_by_key(|t| Reverse(t.len()));

    let mut result = content.to_string();

    for title in titles {
        result = replace_with_wikilink(&result, title);
    }

    result
}

/// Replace all case-insensitive occurrences of `title` in `text` with `[[Title]]`.
/// Avoids replacing text already inside `[[...]]` or inside inline code.
/// Preserves the original matched casing via a display alias: `[[Title|title]]`.
fn replace_with_wikilink(text: &str, title: &str) -> String {
    let title_lower = title.to_lowercase();
    let title_chars: Vec<char> = title_lower.chars().collect();
    let m = title_chars.len();
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();

    let mut result = String::with_capacity(text.len() + 32);
    let mut i = 0;

    while i < n {
        // Skip content inside [[...]] — don't double-link
        if i + 1 < n && chars[i] == '[' && chars[i + 1] == '[' {
            // Copy everything until matching ]]
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
            while i < n {
                if i + 1 < n && chars[i] == ']' && chars[i + 1] == ']' {
                    result.push(chars[i]);
                    result.push(chars[i + 1]);
                    i += 2;
                    break;
                }
                result.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Skip content inside backtick code spans
        if chars[i] == '`' {
            result.push(chars[i]);
            i += 1;
            while i < n && chars[i] != '`' {
                result.push(chars[i]);
                i += 1;
            }
            if i < n {
                result.push(chars[i]); // closing backtick
                i += 1;
            }
            continue;
        }

        // Try to match title at position i
        if i + m <= n {
            let window_lower: String = chars[i..i + m].iter()
                .collect::<String>()
                .to_lowercase();
            if window_lower == title_lower {
                let before_ok = i == 0 || !chars[i - 1].is_alphanumeric();
                let after_ok = i + m >= n || !chars[i + m].is_alphanumeric();
                if before_ok && after_ok {
                    let matched_text: String = chars[i..i + m].iter().collect();
                    let link = if matched_text == title {
                        format!("[[{}]]", title)
                    } else {
                        format!("[[{}|{}]]", title, matched_text)
                    };
                    result.push_str(&link);
                    i += m;
                    continue;
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}
