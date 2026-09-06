use crate::document::types::{Block, BlockKind, DetectedTable, TableRender};
use crate::layout::table_inference::{
    looks_like_key_value_label, normalize_field_label, normalize_field_value, FormField,
    ParsedNumericRow, StructuredRegion, StructuredRegionKind,
};
use crate::render::strings::escape_table_cell;

pub(crate) fn render_table(blocks: &[&Block]) -> String {
    let (grid, max_col) = build_table_grid(blocks);
    if grid.is_empty() {
        return String::new();
    }

    if let Some(rendered) = render_key_value_grid(&grid, max_col + 1) {
        return rendered;
    }

    render_table_grid(&grid, max_col + 1)
}

pub(crate) fn render_coordinate_table(table: &DetectedTable) -> String {
    match &table.render {
        TableRender::Markdown => render_detected_markdown_table(&table.rows),
        TableRender::Layout { text } => {
            let mut rendered = String::from("```text\n");
            rendered.push_str(text.trim_end());
            rendered.push_str("\n```\n");
            rendered
        }
    }
}

pub(crate) fn render_structured_region(
    markdown: &mut String,
    blocks: &[&Block],
    start: usize,
    region: &StructuredRegion,
) {
    match &region.kind {
        StructuredRegionKind::TableCells => {
            markdown.push_str(&render_table(&blocks[start..region.next_index]));
            markdown.push('\n');
        }
        StructuredRegionKind::NumericTable {
            headers,
            rows,
            total,
        } => markdown.push_str(&render_inferred_numeric_table(
            headers,
            rows,
            total.as_deref(),
        )),
        StructuredRegionKind::FormFields { fields } => {
            markdown.push_str(&render_inferred_form_fields(fields));
        }
    }
}

fn build_table_grid(
    blocks: &[&Block],
) -> (
    std::collections::BTreeMap<usize, std::collections::BTreeMap<usize, String>>,
    usize,
) {
    use std::collections::BTreeMap;

    let mut grid: BTreeMap<usize, BTreeMap<usize, String>> = BTreeMap::new();
    let mut max_col = 0usize;

    for block in blocks {
        if let BlockKind::TableCell { row, col } = block.kind {
            grid.entry(row)
                .or_default()
                .insert(col, block.text.trim().to_string());
            max_col = max_col.max(col);
        }
    }

    (grid, max_col)
}

fn render_table_grid(
    grid: &std::collections::BTreeMap<usize, std::collections::BTreeMap<usize, String>>,
    col_count: usize,
) -> String {
    let mut result = String::new();
    let mut first_row = true;

    for row_cells in grid.values() {
        result.push('|');
        for col in 0..col_count {
            let cell = row_cells.get(&col).map(String::as_str).unwrap_or("");
            result.push_str(&format!(" {} |", cell));
        }
        result.push('\n');

        // Insert separator after header row
        if first_row {
            result.push('|');
            for _ in 0..col_count {
                result.push_str(" --- |");
            }
            result.push('\n');
            first_row = false;
        }
    }

    result
}

fn render_key_value_grid(
    grid: &std::collections::BTreeMap<usize, std::collections::BTreeMap<usize, String>>,
    col_count: usize,
) -> Option<String> {
    if col_count != 2 || grid.len() < 2 {
        return None;
    }

    let mut rendered = String::new();
    let mut pair_count = 0usize;

    for row_cells in grid.values() {
        let key = row_cells.get(&0).map(String::as_str).unwrap_or("").trim();
        let value = row_cells.get(&1).map(String::as_str).unwrap_or("").trim();

        if key.is_empty() || value.is_empty() || !looks_like_key_value_label(key) {
            return None;
        }

        rendered.push_str(&format!(
            "- {}: {}\n",
            normalize_field_label(key),
            normalize_field_value(value)
        ));
        pair_count += 1;
    }

    if pair_count >= 2 {
        rendered.push('\n');
        Some(rendered)
    } else {
        None
    }
}

fn render_detected_markdown_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return String::new();
    }

    let mut rendered = String::new();
    for (row_idx, row) in rows.iter().enumerate() {
        rendered.push('|');
        for col in 0..col_count {
            let cell = row.get(col).map(String::as_str).unwrap_or("");
            rendered.push_str(&format!(" {} |", escape_table_cell(cell.trim())));
        }
        rendered.push('\n');
        if row_idx == 0 {
            rendered.push('|');
            for _ in 0..col_count {
                rendered.push_str(" --- |");
            }
            rendered.push('\n');
        }
    }
    rendered
}

fn render_inferred_numeric_table(
    headers: &[String],
    rows: &[ParsedNumericRow],
    total: Option<&str>,
) -> String {
    let mut markdown = String::new();
    markdown.push('|');
    for header in headers {
        markdown.push_str(&format!(" {} |", header));
    }
    markdown.push('\n');
    markdown.push('|');
    for _ in headers {
        markdown.push_str(" --- |");
    }
    markdown.push('\n');

    for parsed in rows {
        markdown.push('|');
        markdown.push_str(&format!(" {} |", escape_table_cell(&parsed.label)));
        for value in &parsed.values {
            markdown.push_str(&format!(" {} |", escape_table_cell(value)));
        }
        markdown.push('\n');
    }

    if let Some(total) = total {
        markdown.push('\n');
        markdown.push_str(&format!("Total: {}\n", total));
    }

    markdown.push('\n');
    markdown
}

fn render_inferred_form_fields(fields: &[FormField]) -> String {
    let mut markdown = String::new();
    for field in fields {
        markdown.push_str(&format!(
            "- {}: {}\n",
            field.label,
            field.value.as_deref().unwrap_or("________")
        ));
    }
    markdown.push('\n');
    markdown
}
