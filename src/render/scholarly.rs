//! Scholarly first-page cleanup and front-matter rendering.

use crate::document::types::{Block, BlockKind, Page};
use crate::render::media::PageMediaPlan;
use crate::render::strings::{
    append_paragraph, append_plain_text, normalize_front_matter_text, normalize_heading_text,
};
use regex::Regex;
use std::sync::OnceLock;

pub(crate) struct ScholarlyFrontMatterRender {
    pub(crate) markdown: String,
    pub(crate) next_index: usize,
}

fn render_heading(text: &str, level: u8) -> String {
    let prefix = "#".repeat(level.clamp(1, 6) as usize);
    format!("{} {}\n\n", prefix, normalize_heading_text(text.trim()))
}

fn scholarly_metadata_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^\s*(arxiv:\S+|doi:\S+|https?://doi\.org/\S+|preprint\b|published as\b|accepted at\b)",
        )
        .unwrap()
    })
}

fn scholarly_note_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(permission to make digital|all rights reserved|copyright held by|acm isbn|provided proper attribution|equal contribution|author contributions listed|correspondence to:|conference on|neurips|nips|icml|iclr|cvpr|eccv|arxiv,\s*\d{4})",
        )
        .unwrap()
    })
}

fn abstract_heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^\s*abstract(?:\s*[:—–-]\s*.*)?\s*$").unwrap())
}

fn numbered_section_heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^\s*\d+(?:\.\d+)?\.?\s*(?:\d+\.?\s*)?(Introduction|Contents|Summary|Background|Methods?|Approach|Related Work|Experiments?|Results?|Conclusion|Overview)\b",
        )
        .unwrap()
    })
}

fn affiliation_keyword_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(university|institute|department|school|college|research|laborator(?:y|ies)|openai|google|microsoft|meta|deepmind|anthropic|stanford|berkeley|toronto|eth zurich|san francisco)\b",
        )
        .unwrap()
    })
}

pub(crate) fn render_scholarly_first_page(
    blocks: &[&Block],
    page: &Page,
    media_plan: &PageMediaPlan,
    mut render_block: impl FnMut(&mut String, &Block, bool),
) -> Option<ScholarlyFrontMatterRender> {
    if page.page_num != 0 || blocks.len() < 3 {
        return None;
    }

    let title_idx = find_scholarly_title_candidate(blocks, page)?;
    let abstract_idx = blocks
        .iter()
        .position(|block| is_abstract_heading(block.text.trim()));
    let metadata_count = blocks
        .iter()
        .take(16)
        .filter(|block| is_scholarly_metadata_line(block.text.trim()))
        .count();

    if abstract_idx.is_none() && metadata_count == 0 {
        return None;
    }

    let end_idx = find_front_matter_end(blocks, abstract_idx).unwrap_or(blocks.len());
    if end_idx <= title_idx {
        return None;
    }

    let mut markdown = String::new();
    markdown.push_str(&render_heading(blocks[title_idx].text.trim(), 1));

    let abstract_boundary = abstract_idx.unwrap_or(end_idx);
    let mut metadata_blocks: Vec<&Block> = Vec::new();
    let mut author_blocks: Vec<&Block> = Vec::new();
    let mut deferred_notes: Vec<&Block> = Vec::new();
    let mut deferred_media: Vec<&Block> = Vec::new();

    for (idx, block) in blocks.iter().enumerate().take(abstract_boundary) {
        if idx == title_idx {
            continue;
        }

        if matches!(
            block.kind,
            BlockKind::Image { .. } | BlockKind::Figure { .. }
        ) {
            deferred_media.push(block);
            continue;
        }

        let text = block.text.trim();
        if text.is_empty() {
            continue;
        }

        match block.kind {
            BlockKind::PageNumber
            | BlockKind::RunningHeader
            | BlockKind::RunningFooter
            | BlockKind::Artifact => {}
            _ if is_scholarly_metadata_line(text) => metadata_blocks.push(block),
            _ if is_scholarly_note_line(text) => deferred_notes.push(block),
            _ if looks_like_author_block(block)
                || matches!(block.kind, BlockKind::TableCell { .. }) =>
            {
                author_blocks.push(block)
            }
            _ => author_blocks.push(block),
        }
    }

    for block in metadata_blocks {
        append_plain_text(
            &mut markdown,
            &normalize_front_matter_text(block.text.trim()),
        );
    }

    for entry in collect_author_entries(&author_blocks) {
        append_plain_text(&mut markdown, &entry);
    }

    if let Some(abstract_idx) = abstract_idx {
        markdown.push_str("## Abstract\n\n");

        if let Some(inline_body) = split_abstract_block(blocks[abstract_idx].text.trim()) {
            append_paragraph(&mut markdown, inline_body);
        }

        for block in blocks.iter().take(end_idx).skip(abstract_idx + 1) {
            if matches!(
                block.kind,
                BlockKind::Image { .. } | BlockKind::Figure { .. }
            ) {
                deferred_media.push(block);
                continue;
            }

            let text = block.text.trim();
            if text.is_empty() {
                continue;
            }
            match block.kind {
                BlockKind::PageNumber
                | BlockKind::RunningHeader
                | BlockKind::RunningFooter
                | BlockKind::Artifact => {}
                _ if is_scholarly_note_line(text) => deferred_notes.push(block),
                _ => render_block(&mut markdown, block, true),
            }
        }
    }

    for block in deferred_notes {
        render_block(&mut markdown, block, true);
    }

    for block in deferred_media {
        if media_plan.kept_block_ids.contains(&block.id) {
            render_block(&mut markdown, block, false);
        }
    }

    Some(ScholarlyFrontMatterRender {
        markdown,
        next_index: end_idx,
    })
}

fn find_scholarly_title_candidate(blocks: &[&Block], page: &Page) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    let y_cutoff = page.height * 0.38;

    for (idx, block) in blocks.iter().enumerate() {
        let text = block.text.trim();
        if text.is_empty() || block.bbox.y0 > y_cutoff {
            continue;
        }
        if is_scholarly_metadata_line(text)
            || is_scholarly_note_line(text)
            || is_abstract_heading(text)
            || is_main_section_heading(text)
            || looks_like_affiliation_line(text)
            || matches!(
                block.kind,
                BlockKind::TableCell { .. }
                    | BlockKind::CoordinateTable { .. }
                    | BlockKind::Image { .. }
                    | BlockKind::Figure { .. }
                    | BlockKind::Formula { .. }
                    | BlockKind::FormulaReview { .. }
                    | BlockKind::PageNumber
                    | BlockKind::RunningHeader
                    | BlockKind::RunningFooter
                    | BlockKind::Artifact
            )
        {
            continue;
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() < 3 && text.len() < 24 {
            continue;
        }

        let mut score = block.font_size * 6.0 - block.bbox.y0 * 0.03;
        if matches!(block.kind, BlockKind::Heading { .. }) {
            score += 20.0;
        }
        if has_title_stopword(text) {
            score += 6.0;
        }
        if text.len() > 140 {
            score -= 8.0;
        }
        if text.chars().filter(|ch| ch.is_ascii_digit()).count() > 8 {
            score -= 12.0;
        }

        match best {
            Some((_, best_score)) if score <= best_score => {}
            _ => best = Some((idx, score)),
        }
    }

    best.map(|(idx, _)| idx)
}

fn find_front_matter_end(blocks: &[&Block], abstract_idx: Option<usize>) -> Option<usize> {
    let start = abstract_idx.map(|idx| idx + 1).unwrap_or(0);
    (start..blocks.len()).find(|&idx| is_main_section_heading(blocks[idx].text.trim()))
}

fn is_scholarly_metadata_line(text: &str) -> bool {
    scholarly_metadata_re().is_match(text)
}

fn is_scholarly_note_line(text: &str) -> bool {
    scholarly_note_re().is_match(text)
        || text.starts_with("^{")
        || text.starts_with('©')
        || text.starts_with("ACM Reference Format:")
}

pub(crate) fn is_abstract_heading(text: &str) -> bool {
    abstract_heading_re().is_match(text) || split_abstract_block(text).is_some()
}

fn split_abstract_block(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("abstract") {
        return None;
    }

    let abstract_prefix = trimmed.get(..8).unwrap_or(trimmed);
    if !abstract_prefix.eq_ignore_ascii_case("abstract") {
        return None;
    }

    let rest = trimmed[8..]
        .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, ':' | '—' | '–' | '-'))
        .trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

fn is_main_section_heading(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    numbered_section_heading_re().is_match(trimmed)
        || lower == "introduction"
        || lower == "contents"
        || lower == "summary"
        || lower.starts_with("1introduction")
        || lower.starts_with("1. introduction")
        || lower.starts_with("1. 1. introduction")
        || lower.starts_with("1 contents")
}

fn looks_like_author_block(block: &Block) -> bool {
    let text = block.text.trim();
    if text.is_empty() {
        return false;
    }

    matches!(block.kind, BlockKind::TableCell { .. })
        || looks_like_author_name_line(text)
        || looks_like_affiliation_line(text)
}

fn looks_like_author_name_line(text: &str) -> bool {
    let words: Vec<&str> = text
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';'))
        .filter(|word| !word.is_empty())
        .collect();
    if !(2..=16).contains(&words.len()) {
        return false;
    }

    let stopwords = [
        "a", "an", "and", "are", "as", "at", "by", "for", "from", "in", "is", "of", "on", "or",
        "the", "to", "with",
    ];
    let capitalized = words
        .iter()
        .filter(|word| {
            word.chars()
                .find(|ch| ch.is_alphabetic())
                .map(|ch| ch.is_uppercase())
                .unwrap_or(false)
        })
        .count();
    let stopword_count = words
        .iter()
        .filter(|word| stopwords.contains(&word.to_ascii_lowercase().as_str()))
        .count();
    let lowercase_content_words = words
        .iter()
        .filter(|word| {
            let lower = word.to_ascii_lowercase();
            !stopwords.contains(&lower.as_str())
                && word
                    .chars()
                    .find(|ch| ch.is_alphabetic())
                    .map(|ch| ch.is_lowercase())
                    .unwrap_or(false)
        })
        .count();

    capitalized * 5 >= words.len() * 4 && stopword_count == 0 && lowercase_content_words == 0
}

fn looks_like_affiliation_line(text: &str) -> bool {
    text.contains('@') || affiliation_keyword_re().is_match(text)
}

fn has_title_stopword(text: &str) -> bool {
    let lower = format!(" {} ", text.to_ascii_lowercase());
    [
        " a ",
        " about ",
        " against ",
        " all ",
        " and ",
        " are ",
        " for ",
        " from ",
        " in ",
        " is ",
        " of ",
        " on ",
        " the ",
        " to ",
        " with ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn collect_author_entries(blocks: &[&Block]) -> Vec<String> {
    let mut entries = Vec::new();
    let mut idx = 0;

    while idx < blocks.len() {
        if matches!(blocks[idx].kind, BlockKind::TableCell { .. }) {
            let mut table_blocks = Vec::new();
            while idx < blocks.len() && matches!(blocks[idx].kind, BlockKind::TableCell { .. }) {
                table_blocks.push(blocks[idx]);
                idx += 1;
            }
            entries.extend(table_cell_author_entries(&table_blocks));
            continue;
        }

        let text = normalize_front_matter_text(blocks[idx].text.trim());
        if !text.is_empty() {
            entries.push(text);
        }
        idx += 1;
    }

    entries.dedup();
    entries
}

fn table_cell_author_entries(blocks: &[&Block]) -> Vec<String> {
    use std::collections::BTreeMap;

    let mut cells = BTreeMap::new();
    for block in blocks {
        if let BlockKind::TableCell { row, col } = block.kind {
            let text = normalize_front_matter_text(block.text.trim());
            if !text.is_empty() {
                cells.insert((row, col), text);
            }
        }
    }

    cells.into_values().collect()
}
