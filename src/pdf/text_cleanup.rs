pub(super) fn cleanup_extracted_text(input: &str) -> String {
    let mut cleaned = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Preserve markdown-relevant layout; normalize CRLF/CR to LF.
            '\r' => {
                if matches!(chars.peek(), Some('\n')) {
                    chars.next();
                }
                cleaned.push('\n');
            }
            '\n' | '\t' => cleaned.push(ch),

            // Strip known PDF/OCR junk that should never render visibly.
            '\u{00AD}' // soft hyphen
            | '\u{200B}' // zero width space
            | '\u{200C}' // zero width non-joiner
            | '\u{200D}' // zero width joiner
            | '\u{2060}' // word joiner
            | '\u{FEFF}' // zero width no-break space / BOM
            | '\u{200E}' // left-to-right mark
            | '\u{200F}' // right-to-left mark
            | '\u{202A}' // bidi embedding/override isolates
            | '\u{202B}'
            | '\u{202C}'
            | '\u{202D}'
            | '\u{202E}'
            | '\u{2066}'
            | '\u{2067}'
            | '\u{2068}'
            | '\u{2069}' => {}

            // Normalize a small, safe subset of spacing and hyphen variants.
            '\u{00A0}' | '\u{202F}' => cleaned.push(' '),
            '\u{2010}' | '\u{2011}' => cleaned.push('-'),

            // Expand common presentation ligatures to their textual form.
            '\u{FB00}' => cleaned.push_str("ff"),
            '\u{FB01}' => cleaned.push_str("fi"),
            '\u{FB02}' => cleaned.push_str("fl"),
            '\u{FB03}' => cleaned.push_str("ffi"),
            '\u{FB04}' => cleaned.push_str("ffl"),
            '\u{FB05}' => cleaned.push_str("st"),
            '\u{FB06}' => cleaned.push_str("st"),

            c if is_bad_control(c) => {}
            _ => cleaned.push(ch),
        }
    }

    cleaned
}

fn is_bad_control(ch: char) -> bool {
    matches!(ch, '\u{0000}'..='\u{0008}' | '\u{000B}' | '\u{000C}' | '\u{000E}'..='\u{001F}' | '\u{007F}'..='\u{009F}')
}

#[cfg(test)]
mod tests {
    use super::cleanup_extracted_text;

    #[test]
    fn strips_control_chars_but_keeps_layout() {
        let input = "alpha\u{0000}\tbeta\u{000C}\ngamma\u{0085}delta\r\nomega";
        let cleaned = cleanup_extracted_text(input);
        assert_eq!(cleaned, "alpha\tbeta\ngammadelta\nomega");
    }

    #[test]
    fn removes_soft_hyphen_artifacts_across_line_breaks() {
        let input = "electro\u{00AD}\nmagnetic and co\u{00AD}operate";
        let cleaned = cleanup_extracted_text(input);
        assert_eq!(cleaned, "electro\nmagnetic and cooperate");
    }

    #[test]
    fn normalizes_safe_unicode_spacing_hyphen_and_ligatures() {
        let input = "state\u{2011}of\u{2010}the\u{00A0}art \u{FB01}le \u{FB03}eld";
        let cleaned = cleanup_extracted_text(input);
        assert_eq!(cleaned, "state-of-the art file ffield");
    }

    #[test]
    fn preserves_math_minus_and_em_dash() {
        let input = "x \u{2212} y \u{2014} z";
        let cleaned = cleanup_extracted_text(input);
        assert_eq!(cleaned, input);
    }

    #[test]
    fn strips_zero_width_and_bidi_artifacts() {
        let input = "A\u{200B}B\u{2060}C\u{202A}D\u{202C}E";
        let cleaned = cleanup_extracted_text(input);
        assert_eq!(cleaned, "ABCDE");
    }
}
