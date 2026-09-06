//! String normalization, escaping, and Markdown text helpers.

pub(crate) fn escape_comment_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace("--", "- -")
}

pub(crate) fn normalize_heading_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    let chars: Vec<char> = text.chars().collect();

    for (idx, ch) in chars.iter().copied().enumerate() {
        let prev = idx.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(idx + 1).copied();

        let should_insert_space = match (prev, ch, next) {
            (Some(prev), ch, Some(next))
                if prev.is_ascii_digit()
                    && !prev.is_whitespace()
                    && ch.is_uppercase()
                    && next.is_lowercase() =>
            {
                let prev_prev = idx.checked_sub(2).and_then(|i| chars.get(i)).copied();
                !matches!(prev_prev, Some(p) if p.is_alphabetic())
            }
            (Some('.'), ch, Some(next)) if ch.is_uppercase() && next.is_lowercase() => {
                let prev_prev = idx.checked_sub(2).and_then(|i| chars.get(i)).copied();
                matches!(prev_prev, Some(p) if p.is_ascii_digit())
            }
            _ => false,
        };

        if should_insert_space && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(ch);
    }

    out
}

pub(crate) fn escape_table_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

pub(crate) fn normalize_front_matter_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn append_paragraph(markdown: &mut String, text: &str) {
    let text = normalize_paragraph_text(text);
    if text.is_empty() {
        return;
    }
    markdown.push_str(&text);
    markdown.push_str("\n\n");
}

pub(crate) fn append_plain_text(markdown: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    markdown.push_str(text);
    markdown.push_str("\n\n");
}

/// Wrap rendered block text in Markdown inline markers when the source
/// block's dominant font is bold and/or italic. Returns the text unchanged
/// when both flags are false. Trims leading/trailing whitespace before
/// wrapping so the markers sit flush against the text.
pub(crate) fn inline_wrap(text: &str, bold: bool, italic: bool) -> String {
    if !bold && !italic {
        return text.to_string();
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let marker = match (bold, italic) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => unreachable!(),
    };
    format!("{marker}{trimmed}{marker}")
}

pub(crate) fn normalize_paragraph_text(text: &str) -> String {
    text.replace("-\n", "-")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
