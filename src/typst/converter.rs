//! Markdown → Typst converter using pulldown-cmark events.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::config::TypstConfig;
use super::latex2typst::latex_to_typst;

/// Convert Markdown text to Typst markup.
pub fn convert(md_text: &str, config: &TypstConfig) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH;
    let parser = Parser::new_ext(md_text, options);
    let mut renderer = TypstRenderer::new(config);
    for event in parser {
        renderer.process(event);
    }
    renderer.finish()
}

// ---------------------------------------------------------------------------
// Typst escape
// ---------------------------------------------------------------------------

fn escape_typst(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '*' => out.push_str("\\*"),
            '_' => out.push_str("\\_"),
            '#' => out.push_str("\\#"),
            '@' => out.push_str("\\@"),
            '~' => out.push_str("\\~"),
            '<' => out.push_str("\\<"),
            '>' => out.push_str("\\>"),
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Renderer state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum ListKind {
    Bullet,
    Ordered,
}

/// Tracks what context we're writing into.
#[derive(Debug, Clone, Copy, PartialEq)]
enum WriteTarget {
    /// Writing to main output
    Main,
    /// Writing into a heading's inline buffer
    Heading,
    /// Writing into a table cell buffer
    TableCell,
    /// Writing into a blockquote's inner buffer
    Blockquote,
}

struct TypstRenderer<'a> {
    config: &'a TypstConfig,
    output: String,

    // Heading state
    heading_level: Option<HeadingLevel>,
    heading_buf: String,

    // List state
    list_stack: Vec<ListKind>,
    item_first_para: bool,

    // Blockquote state — nested rendering via separate buffers
    blockquote_stack: Vec<String>,

    // Table state
    table_alignments: Vec<Alignment>,
    in_table_head: bool,
    header_cells: Vec<String>,
    body_rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    cell_buf: String,
    in_table: bool,

    // Inline formatting tracking for proper nesting
    write_target: WriteTarget,
}

impl<'a> TypstRenderer<'a> {
    fn new(config: &'a TypstConfig) -> Self {
        Self {
            config,
            output: String::new(),
            heading_level: None,
            heading_buf: String::new(),
            list_stack: Vec::new(),
            item_first_para: false,
            blockquote_stack: Vec::new(),
            table_alignments: Vec::new(),
            in_table_head: false,
            header_cells: Vec::new(),
            body_rows: Vec::new(),
            current_row: Vec::new(),
            cell_buf: String::new(),
            in_table: false,
            write_target: WriteTarget::Main,
        }
    }

    fn write(&mut self, s: &str) {
        // When inside a blockquote but not in a special target, write to blockquote buffer
        match self.write_target {
            WriteTarget::Main | WriteTarget::Blockquote => {
                if let Some(bq) = self.blockquote_stack.last_mut() {
                    bq.push_str(s);
                } else {
                    self.output.push_str(s);
                }
            }
            WriteTarget::Heading => self.heading_buf.push_str(s),
            WriteTarget::TableCell => self.cell_buf.push_str(s),
        }
    }

    fn process(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.open_tag(tag),
            Event::End(tag) => self.close_tag(tag),
            Event::Text(text) => self.handle_text(&text),
            Event::Code(code) => self.handle_code(&code),
            Event::SoftBreak => self.write(" "),
            Event::HardBreak => self.write("\\\n"),
            Event::Rule => {
                let style = &self.config.hr.style;
                let s = format!("{style}\n\n");
                self.write(&s);
            }
            Event::Html(html) => {
                let s = format!("/* HTML: {} */\n\n", html.trim());
                self.write(&s);
            }
            Event::InlineHtml(html) => {
                let s = format!("/* {} */", html.trim());
                self.write(&s);
            }
            Event::InlineMath(math) => {
                let converted = latex_to_typst(&math);
                let s = format!("${converted}$");
                self.write(&s);
            }
            Event::DisplayMath(math) => {
                let converted = latex_to_typst(&math);
                let s = format!("$ {converted} $\n\n");
                self.write(&s);
            }
            Event::TaskListMarker(_) | Event::FootnoteReference(_) => {}
        }
    }

    fn open_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.heading_level = Some(level);
                self.heading_buf.clear();
                self.write_target = WriteTarget::Heading;
            }
            Tag::Paragraph => {
                // Paragraph content is collected via text events;
                // formatting is applied at paragraph close.
            }
            Tag::Strong => self.write("*"),
            Tag::Emphasis => self.write("_"),
            Tag::Strikethrough => self.write("#strike["),
            Tag::Link { dest_url, .. } => {
                let s = format!("#link(\"{dest_url}\")[");
                self.write(&s);
            }
            Tag::Image { dest_url, title, .. } => {
                let width_arg = if self.config.image.width.is_empty() {
                    String::new()
                } else {
                    format!(", width: {}", self.config.image.width)
                };
                if self.config.image.use_figure {
                    // Alt text arrives as Text events; we handle at close
                    let s = format!("#figure(image(\"{dest_url}\"{width_arg}), caption: [");
                    self.write(&s);
                } else {
                    let s = format!("#image(\"{dest_url}\"{width_arg})");
                    self.write(&s);
                    // Skip caption for bare image — consume alt text but don't wrap
                    if !title.is_empty() {
                        // title is separate from alt
                    }
                }
            }
            Tag::List(first_item) => {
                let kind = if first_item.is_some() {
                    ListKind::Ordered
                } else {
                    ListKind::Bullet
                };
                self.list_stack.push(kind);
            }
            Tag::Item => {
                self.item_first_para = true;
            }
            Tag::BlockQuote(_) => {
                self.blockquote_stack.push(String::new());
            }
            Tag::CodeBlock(kind) => {
                let info = match &kind {
                    CodeBlockKind::Fenced(info) => {
                        let s = info.trim().to_string();
                        if s.is_empty() { None } else { Some(s) }
                    }
                    CodeBlockKind::Indented => None,
                };
                let func = &self.config.code.block_function;
                if !func.is_empty() {
                    let lang_attr = info
                        .as_ref()
                        .map(|i| format!("lang: \"{i}\", "))
                        .unwrap_or_default();
                    let s = format!("#{func}({lang_attr}```\n");
                    self.write(&s);
                } else if let Some(info) = &info {
                    let s = format!("```{info}\n");
                    self.write(&s);
                } else {
                    self.write("```\n");
                }
            }
            Tag::Table(alignments) => {
                self.table_alignments = alignments;
                self.in_table = true;
                self.in_table_head = false;
                self.header_cells.clear();
                self.body_rows.clear();
                self.current_row.clear();
            }
            Tag::TableHead => {
                self.in_table_head = true;
            }
            Tag::TableRow => {
                self.current_row.clear();
            }
            Tag::TableCell => {
                self.cell_buf.clear();
                self.write_target = WriteTarget::TableCell;
            }
            _ => {}
        }
    }

    fn close_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(level) => {
                self.write_target = WriteTarget::Main;
                let prefix = "=".repeat(heading_level_num(level) as usize);
                let content = std::mem::take(&mut self.heading_buf);
                let s = format!("\n{prefix} {content}\n\n");
                self.write(&s);
                self.heading_level = None;
            }
            TagEnd::Paragraph => {
                if !self.list_stack.is_empty() && self.item_first_para {
                    // First paragraph in a list item — marker was already written
                    // The newline comes from the text event handling
                } else if !self.list_stack.is_empty() {
                    // Subsequent paragraph in list item
                } else {
                    self.write("\n");
                }
            }
            TagEnd::Strong => self.write("*"),
            TagEnd::Emphasis => self.write("_"),
            TagEnd::Strikethrough => self.write("]"),
            TagEnd::Link => self.write("]"),
            TagEnd::Image => {
                if self.config.image.use_figure {
                    self.write("])");
                }
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.write("\n");
                }
            }
            TagEnd::Item => {}
            TagEnd::BlockQuote(_) => {
                let inner = self.blockquote_stack.pop().unwrap_or_default();
                let body = inner.trim();
                let func = &self.config.blockquote.function;
                let s = format!("#{func}[\n{body}\n]\n\n");
                self.write(&s);
            }
            TagEnd::CodeBlock => {
                let func = &self.config.code.block_function;
                if !func.is_empty() {
                    self.write("```)\n\n");
                } else {
                    self.write("```\n\n");
                }
            }
            TagEnd::Table => {
                self.write_target = WriteTarget::Main;
                let table = self.format_table();
                self.write(&table);
                self.in_table = false;
            }
            TagEnd::TableHead => {
                // pulldown-cmark puts header cells directly in TableHead (no TableRow wrapper)
                self.header_cells = std::mem::take(&mut self.current_row);
                self.in_table_head = false;
            }
            TagEnd::TableRow => {
                // Body rows only (header is handled at TableHead close)
                let row = std::mem::take(&mut self.current_row);
                self.body_rows.push(row);
            }
            TagEnd::TableCell => {
                let content = std::mem::take(&mut self.cell_buf);
                self.current_row.push(content);
                self.write_target = if self.blockquote_stack.is_empty() {
                    WriteTarget::Main
                } else {
                    WriteTarget::Blockquote
                };
            }
            _ => {}
        }
    }

    fn handle_text(&mut self, text: &str) {
        // Inside code blocks, don't escape
        if self.write_target == WriteTarget::TableCell
            || self.write_target == WriteTarget::Heading
        {
            let escaped = escape_typst(text);
            self.write(&escaped);
            return;
        }

        // Check if we're in a list paragraph
        if !self.list_stack.is_empty() && self.item_first_para {
            let depth = self.list_stack.len() - 1;
            let indent = "  ".repeat(depth);
            let marker = match self.list_stack.last() {
                Some(ListKind::Ordered) => "+",
                Some(ListKind::Bullet) | None => "-",
            };
            let escaped = escape_typst(text);
            let s = format!("{indent}{marker} {escaped}\n");
            self.write(&s);
            self.item_first_para = false;
            return;
        }

        if !self.list_stack.is_empty() {
            // Continuation text in list
            let escaped = escape_typst(text);
            self.write(&escaped);
            return;
        }

        let escaped = escape_typst(text);
        self.write(&escaped);
    }

    fn handle_code(&mut self, code: &str) {
        let s = format!("`{code}`");
        self.write(&s);
    }

    fn format_table(&self) -> String {
        let cols = if !self.header_cells.is_empty() {
            self.header_cells.len()
        } else if let Some(first_row) = self.body_rows.first() {
            first_row.len()
        } else {
            1
        };

        let col_spec: String = self
            .table_alignments
            .iter()
            .take(cols)
            .map(|a| match a {
                Alignment::None => "auto",
                Alignment::Left => "left",
                Alignment::Center => "center",
                Alignment::Right => "right",
            })
            .collect::<Vec<_>>()
            .join(", ");

        let mut lines = vec!["#table(".to_string()];
        lines.push(format!("  columns: {cols},"));
        lines.push(format!("  align: ({col_spec},),"));

        if !self.config.table.stroke.is_empty() {
            lines.push(format!("  stroke: {},", self.config.table.stroke));
        }

        if !self.header_cells.is_empty() {
            lines.push("  table.header(".to_string());
            for cell in &self.header_cells {
                if self.config.table.header_bold {
                    lines.push(format!("    [*{cell}*],"));
                } else {
                    lines.push(format!("    [{cell}],"));
                }
            }
            lines.push("  ),".to_string());
        }

        for row in &self.body_rows {
            for cell in row {
                lines.push(format!("  [{cell}],"));
            }
        }
        lines.push(")\n".to_string());

        lines.join("\n")
    }

    fn finish(self) -> String {
        let trimmed = self.output.trim();
        if trimmed.is_empty() {
            "\n".to_string()
        } else {
            format!("{trimmed}\n")
        }
    }
}

fn heading_level_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typst::config::{BlockquoteConfig, ImageConfig, TableConfig};

    fn cvt(md: &str) -> String {
        convert(md, &TypstConfig::default())
    }

    // --- Headings ---

    #[test]
    fn test_headings() {
        for (md, prefix) in [
            ("# H1", "= H1"),
            ("## H2", "== H2"),
            ("### H3", "=== H3"),
            ("#### H4", "==== H4"),
            ("##### H5", "===== H5"),
            ("###### H6", "====== H6"),
        ] {
            assert!(cvt(md).contains(prefix), "expected {prefix} in {}", cvt(md));
        }
    }

    #[test]
    fn test_heading_with_formatting() {
        let result = cvt("## **bold** heading");
        assert!(result.contains("*bold*"));
        assert!(result.contains("=="));
    }

    // --- Inline formatting ---

    #[test]
    fn test_bold() {
        assert!(cvt("**bold**").contains("*bold*"));
    }

    #[test]
    fn test_italic() {
        assert!(cvt("_italic_").contains("_italic_"));
    }

    #[test]
    fn test_strikethrough() {
        assert!(cvt("~~strike~~").contains("#strike[strike]"));
    }

    #[test]
    fn test_inline_code() {
        assert!(cvt("`code`").contains("`code`"));
    }

    #[test]
    fn test_link() {
        let result = cvt("[text](https://example.com)");
        assert!(result.contains("#link(\"https://example.com\")[text]"));
    }

    #[test]
    fn test_image() {
        let result = cvt("![Alt text](photo.jpg)");
        assert!(result.contains("#figure(image(\"photo.jpg\")"));
        assert!(result.contains("caption: [Alt text]"));
    }

    #[test]
    fn test_image_no_figure() {
        let config = TypstConfig {
            image: ImageConfig {
                use_figure: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = convert("![alt](img.png)", &config);
        assert!(result.contains("#image(\"img.png\")"));
        assert!(!result.contains("figure"));
    }

    #[test]
    fn test_image_width() {
        let config = TypstConfig {
            image: ImageConfig {
                width: "80%".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = convert("![alt](img.png)", &config);
        assert!(result.contains("width: 80%"));
    }

    // --- Math ---

    #[test]
    fn test_inline_math() {
        let result = cvt("$x^2$");
        assert!(result.contains("$x^2$"));
    }

    #[test]
    fn test_display_math() {
        let result = cvt("$$\nx = 1\n$$");
        assert!(result.contains("$ x = 1 $"));
    }

    // --- Code blocks ---

    #[test]
    fn test_fenced_code_with_lang() {
        let result = cvt("```python\nprint(1)\n```");
        assert!(result.contains("```python"));
        assert!(result.contains("print(1)"));
    }

    #[test]
    fn test_fenced_code_no_lang() {
        let result = cvt("```\nplain text\n```");
        assert!(result.contains("```\nplain text\n```"));
    }

    // --- Lists ---

    #[test]
    fn test_unordered_list() {
        let result = cvt("- one\n- two\n- three");
        assert!(result.contains("- one"));
        assert!(result.contains("- two"));
        assert!(result.contains("- three"));
    }

    #[test]
    fn test_ordered_list() {
        let result = cvt("1. one\n2. two");
        assert!(result.contains("+ one"));
        assert!(result.contains("+ two"));
    }

    #[test]
    fn test_nested_list() {
        let result = cvt("- top\n  - nested\n- back");
        assert!(result.contains("- top"));
        assert!(result.contains("  - nested"));
        assert!(result.contains("- back"));
    }

    // --- Blockquotes ---

    #[test]
    fn test_blockquote() {
        let result = cvt("> hello");
        assert!(result.contains("#quote[\nhello\n]"));
    }

    #[test]
    fn test_nested_blockquote() {
        let result = cvt("> > nested");
        assert!(result.contains("#quote["));
        assert_eq!(result.matches("#quote[").count(), 2);
    }

    #[test]
    fn test_blockquote_custom_function() {
        let config = TypstConfig {
            blockquote: BlockquoteConfig {
                function: "callout".to_string(),
            },
            ..Default::default()
        };
        let result = convert("> hello", &config);
        assert!(result.contains("#callout["));
    }

    // --- Tables ---

    #[test]
    fn test_simple_table() {
        let result = cvt("| A | B |\n|---|---|\n| 1 | 2 |");
        assert!(result.contains("#table("));
        assert!(result.contains("columns: 2"));
        assert!(result.contains("table.header("));
        assert!(result.contains("[*A*]"));
        assert!(result.contains("[1]"));
    }

    #[test]
    fn test_table_alignment() {
        let result = cvt("| L | C | R |\n|:--|:-:|--:|\n| a | b | c |");
        assert!(result.contains("left"));
        assert!(result.contains("center"));
        assert!(result.contains("right"));
    }

    #[test]
    fn test_table_no_bold_headers() {
        let config = TypstConfig {
            table: TableConfig {
                header_bold: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = convert("| A |\n|---|\n| 1 |", &config);
        assert!(result.contains("[A]"));
        assert!(!result.contains("[*A*]"));
    }

    #[test]
    fn test_table_stroke() {
        let config = TypstConfig {
            table: TableConfig {
                stroke: "0.5pt".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = convert("| A |\n|---|\n| 1 |", &config);
        assert!(result.contains("stroke: 0.5pt"));
    }

    // --- Horizontal rule ---

    #[test]
    fn test_hr() {
        assert!(cvt("---").contains("#line(length: 100%)"));
    }

    // --- HTML ---

    #[test]
    fn test_html_block() {
        let result = cvt("<div>block</div>");
        assert!(result.contains("/* HTML:"));
    }

    // --- Special characters ---

    #[test]
    fn test_escape_special_chars() {
        let result = cvt("# $ * _ @ ~ < >");
        assert!(result.contains("\\$"));
        assert!(result.contains("\\*"));
        assert!(result.contains("\\@"));
    }

    // --- Soft breaks ---

    #[test]
    fn test_softbreak() {
        let result = cvt("line one\nline two");
        assert!(result.contains("line one line two"));
    }
}
