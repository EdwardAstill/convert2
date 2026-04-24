//! Translate LaTeX math expressions to Typst math syntax.
//!
//! Pipeline stages (must run in this order):
//! 1. Environments — \begin{aligned}, matrices, cases
//! 2. Structured commands — \frac, \sqrt, accents, font commands, \mathbb
//! 3. Simple symbol replacements — sorted longest-first
//! 4. Script conversion — _{...} → _(...), ^{...} → ^(...)
//! 5. Multi-character identifier quoting

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

/// Convert a LaTeX math string (without outer `$` delimiters) to Typst math syntax.
pub fn latex_to_typst(latex: &str) -> String {
    let mut s = latex.trim().to_string();
    s = translate_environments(&s);
    s = translate_commands(&s);
    s = translate_scripts(&s);
    s = quote_multichar_identifiers(&s);
    s = collapse_spaces(&s);
    s.trim().to_string()
}

// ---------------------------------------------------------------------------
// Stage 1: Environments
// ---------------------------------------------------------------------------

static RE_ALIGNED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)\\begin\{(?:aligned|align\*?)\}(.*?)\\end\{(?:aligned|align\*?)\}").unwrap()
});
static RE_CASES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)\\begin\{cases\}(.*?)\\end\{cases\}").unwrap()
});

struct MatrixDef {
    pattern: Regex,
    delim: &'static str,
}

static MATRIX_DEFS: LazyLock<Vec<MatrixDef>> = LazyLock::new(|| {
    [
        ("pmatrix", r#""(""#),
        ("bmatrix", r#""[""#),
        ("vmatrix", r#""|""#),
        ("matrix", r#""(""#),
    ]
    .into_iter()
    .map(|(env, delim)| MatrixDef {
        pattern: Regex::new(&format!(r"(?s)\\begin\{{{env}\}}(.*?)\\end\{{{env}\}}")).unwrap(),
        delim,
    })
    .collect()
});

fn translate_environments(s: &str) -> String {
    let mut s = RE_ALIGNED
        .replace_all(s, |caps: &regex::Captures| convert_aligned(&caps[1]))
        .into_owned();
    s = RE_CASES
        .replace_all(&s, |caps: &regex::Captures| convert_cases(&caps[1]))
        .into_owned();
    for def in MATRIX_DEFS.iter() {
        let delim = def.delim;
        s = def
            .pattern
            .replace_all(&s, |caps: &regex::Captures| convert_matrix(&caps[1], delim))
            .into_owned();
    }
    s
}

fn convert_aligned(body: &str) -> String {
    let lines: Vec<String> = body
        .trim()
        .split(r"\\")
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().replace('&', ""))
        .collect();
    lines.join(" \\\n")
}

fn convert_cases(body: &str) -> String {
    let parts: Vec<String> = body
        .trim()
        .split(r"\\")
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let line = line.trim();
            let chunks: Vec<&str> = line.splitn(2, '&').collect();
            let val = chunks[0].trim();
            let cond = chunks.get(1).map(|c| c.trim()).unwrap_or("");
            if cond.is_empty() {
                format!("  {val}")
            } else {
                format!("  {val} & {cond}")
            }
        })
        .collect();
    format!("cases(\n{}\n)", parts.join(",\n"))
}

fn convert_matrix(body: &str, delim: &str) -> String {
    let row_strs: Vec<String> = body
        .trim()
        .split(r"\\")
        .filter(|r| !r.trim().is_empty())
        .map(|row| {
            row.split('&')
                .map(|c| c.trim())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();
    format!("mat(delim: {}, {})", delim, row_strs.join("; "))
}

// ---------------------------------------------------------------------------
// Stage 2 & 3: Structured + simple command replacements
// ---------------------------------------------------------------------------

static RE_TEXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\text\{([^}]*)\}").unwrap());
static RE_MATHBB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\mathbb\{([^}]*)\}").unwrap());
static RE_SQRT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\sqrt").unwrap());

fn translate_commands(s: &str) -> String {
    let mut s = s.to_string();

    // Passthrough: \boxed{x} → x
    s = replace_cmd_passthrough(&s, "boxed");

    // Two-arg commands
    for (latex_name, typst_name) in [
        ("frac", "frac"),
        ("dfrac", "frac"),
        ("tfrac", "frac"),
        ("binom", "binom"),
    ] {
        s = replace_cmd_two_args(&s, latex_name, typst_name);
    }

    // Square root
    s = replace_sqrt(&s);

    // One-arg accent/decoration commands
    for (latex_name, typst_name) in [
        ("hat", "hat"),
        ("bar", "macron"),
        ("vec", "arrow"),
        ("dot", "dot"),
        ("ddot", "diaer"),
        ("tilde", "tilde"),
        ("overline", "overline"),
        ("underline", "underline"),
        ("overbrace", "overbrace"),
        ("underbrace", "underbrace"),
    ] {
        s = replace_cmd_one_arg(&s, latex_name, typst_name);
    }

    // \text{...} → "..."
    s = RE_TEXT
        .replace_all(&s, |caps: &regex::Captures| format!("\"{}\"", &caps[1]))
        .into_owned();

    // Font commands
    for (latex_name, typst_name) in [
        ("mathrm", "upright"),
        ("operatorname", "op"),
        ("mathbf", "bold"),
        ("boldsymbol", "bold"),
        ("mathit", "italic"),
        ("mathcal", "cal"),
    ] {
        s = replace_cmd_one_arg(&s, latex_name, typst_name);
    }

    // \mathbb{X} → XX or bb(X)
    s = RE_MATHBB
        .replace_all(&s, |caps: &regex::Captures| {
            let letter = &caps[1];
            match letter {
                "R" => "RR".to_string(),
                "N" => "NN".to_string(),
                "Z" => "ZZ".to_string(),
                "Q" => "QQ".to_string(),
                "C" => "CC".to_string(),
                "F" => "FF".to_string(),
                "P" => "PP".to_string(),
                "E" => "EE".to_string(),
                "1" => "bb(1)".to_string(),
                _ => format!("bb({letter})"),
            }
        })
        .into_owned();

    // Simple command replacements (sorted longest-first)
    for &(needle, replacement) in SORTED_COMMANDS.iter() {
        s = replace_with_spacing(&s, needle, replacement);
    }

    s
}

// ---------------------------------------------------------------------------
// Command replacement helpers
// ---------------------------------------------------------------------------

fn replace_with_spacing(s: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return s.to_string();
    }
    let mut result = String::with_capacity(s.len());
    let mut remaining = s;
    while let Some(pos) = remaining.find(needle) {
        result.push_str(&remaining[..pos]);
        // Check char before match in the original (unmodified) remaining text
        let before_alpha = pos > 0 && remaining.as_bytes()[pos - 1].is_ascii_alphabetic();
        if before_alpha {
            result.push(' ');
        }
        result.push_str(replacement);
        remaining = &remaining[pos + needle.len()..];
        // Check char after match
        if !remaining.is_empty() && remaining.as_bytes()[0].is_ascii_alphabetic() {
            result.push(' ');
        }
    }
    result.push_str(remaining);
    result
}

fn replace_cmd_passthrough(s: &str, latex_name: &str) -> String {
    let needle = format!("\\{latex_name}{{");
    let mut s = s.to_string();
    while let Some(pos) = s.find(&needle) {
        let brace_start = pos + needle.len() - 1; // position of '{'
        let (arg, end) = extract_braced(&s, brace_start);
        let Some(arg) = arg else { break };
        s = format!("{}{}{}", &s[..pos], arg, &s[end..]);
    }
    s
}

fn replace_cmd_one_arg(s: &str, latex_name: &str, typst_name: &str) -> String {
    let needle = format!("\\{latex_name}{{");
    let mut s = s.to_string();
    while let Some(pos) = s.find(&needle) {
        let brace_start = pos + needle.len() - 1;
        let (arg, end) = extract_braced(&s, brace_start);
        let Some(arg) = arg else { break };
        let prefix = if pos > 0 && s.as_bytes()[pos - 1].is_ascii_alphabetic() {
            " "
        } else {
            ""
        };
        s = format!("{}{prefix}{typst_name}({arg}){}", &s[..pos], &s[end..]);
    }
    s
}

fn replace_cmd_two_args(s: &str, latex_name: &str, typst_name: &str) -> String {
    let needle = format!("\\{latex_name}{{");
    let mut s = s.to_string();
    while let Some(pos) = s.find(&needle) {
        let brace_start = pos + needle.len() - 1;
        let (arg1, after1) = extract_braced(&s, brace_start);
        let Some(arg1) = arg1 else { break };
        let (arg2, after2) = extract_braced(&s, after1);
        let Some(arg2) = arg2 else { break };
        let prefix = if pos > 0 && s.as_bytes()[pos - 1].is_ascii_alphabetic() {
            " "
        } else {
            ""
        };
        s = format!(
            "{}{prefix}{typst_name}({arg1}, {arg2}){}",
            &s[..pos],
            &s[after2..]
        );
    }
    s
}

fn replace_sqrt(s: &str) -> String {
    let mut s = s.to_string();
    while let Some(m) = RE_SQRT.find(&s) {
        let start = m.start();
        let pos = m.end();
        let bytes = s.as_bytes();
        if pos < bytes.len() && bytes[pos] == b'[' {
            // \sqrt[n]{x} → root(n, x)
            let Some(bracket_end) = s[pos..].find(']').map(|i| i + pos) else {
                break;
            };
            let n_arg = &s[pos + 1..bracket_end];
            let (arg, after) = extract_braced(&s, bracket_end + 1);
            let Some(arg) = arg else { break };
            s = format!("{}root({n_arg}, {arg}){}", &s[..start], &s[after..]);
        } else if pos < bytes.len() && bytes[pos] == b'{' {
            // \sqrt{x} → sqrt(x)
            let (arg, after) = extract_braced(&s, pos);
            let Some(arg) = arg else { break };
            s = format!("{}sqrt({arg}){}", &s[..start], &s[after..]);
        } else {
            break;
        }
    }
    s
}

/// Extract content from balanced braces starting at `pos`.
/// Returns `(Some(content), end_pos)` on success or `(None, pos)` on failure.
fn extract_braced(s: &str, mut pos: usize) -> (Option<String>, usize) {
    let bytes = s.as_bytes();
    // Skip leading spaces
    while pos < bytes.len() && bytes[pos] == b' ' {
        pos += 1;
    }
    if pos >= bytes.len() || bytes[pos] != b'{' {
        return (None, pos);
    }
    let mut depth: i32 = 0;
    let start = pos + 1;
    let mut i = pos;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && (bytes[i + 1] == b'{' || bytes[i + 1] == b'}')
        {
            i += 2;
            continue;
        }
        if bytes[i] == b'{' {
            depth += 1;
        } else if bytes[i] == b'}' {
            depth -= 1;
            if depth == 0 {
                return (Some(s[start..i].to_string()), i + 1);
            }
        }
        i += 1;
    }
    (None, pos)
}

// ---------------------------------------------------------------------------
// Stage 4: Script conversion
// ---------------------------------------------------------------------------

fn translate_scripts(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len()
            && (bytes[i] == b'_' || bytes[i] == b'^')
            && bytes[i + 1] == b'{'
        {
            let marker = bytes[i] as char;
            let (content, end) = extract_braced(s, i + 1);
            if let Some(content) = content {
                result.push(marker);
                result.push('(');
                result.push_str(&content);
                result.push(')');
                i = end;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// Stage 5: Multi-character identifier quoting
// ---------------------------------------------------------------------------

static TYPST_MATH_IDENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        // Typst math functions
        "frac", "sqrt", "root", "binom", "boxed", "hat", "macron", "arrow", "diaer", "tilde",
        "overline", "underline", "overbrace", "underbrace", "upright", "bold", "italic", "cal",
        "op", "bb", "mat", "cases", "delim",
        // Math operator functions
        "lim", "sup", "inf", "min", "max", "log", "ln", "exp", "sin", "cos", "tan", "sec", "csc",
        "cot", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "det", "dim", "ker", "deg",
        "arg", "gcd", "mod",
        // Symbols
        "dot", "times", "div", "compose", "ast", "star", "approx", "equiv", "prop", "subset",
        "supset", "forall", "exists", "sum", "product", "integral", "union", "sect", "nabla",
        "infinity", "diff", "ell", "aleph", "nothing", "dots", "quad", "wide", "thin", "med",
        "Re", "Im", "and", "or", "not", "in", "plus", "minus",
        // Greek lowercase
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu", "xi", "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi",
        "omega",
        // Greek uppercase
        "Gamma", "Delta", "Theta", "Lambda", "Xi", "Pi", "Sigma", "Upsilon", "Phi", "Psi",
        "Omega",
        // Blackboard bold
        "RR", "NN", "ZZ", "QQ", "CC", "FF", "PP", "EE",
    ]
    .into_iter()
    .collect()
});

fn quote_multichar_identifiers(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        // Pass already-quoted strings through
        if bytes[i] == b'"' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                j += 1;
            }
            result.push_str(&s[i..=j.min(bytes.len() - 1)]);
            i = j + 1;
            continue;
        }
        // Collect a run of letters
        if (bytes[i] as char).is_alphabetic() {
            let mut j = i;
            while j < bytes.len() && (bytes[j] as char).is_alphabetic() {
                j += 1;
            }
            let word = &s[i..j];
            // Skip whitespace, check for function call
            let mut k = j;
            while k < bytes.len() && bytes[k] == b' ' {
                k += 1;
            }
            let is_func_call = k < bytes.len() && bytes[k] == b'(';
            let is_dot_modifier = i > 0 && bytes[i - 1] == b'.';

            if word.len() >= 2
                && !TYPST_MATH_IDENTS.contains(word)
                && !is_func_call
                && !is_dot_modifier
            {
                result.push('"');
                result.push_str(word);
                result.push('"');
            } else {
                result.push_str(word);
            }
            i = j;
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// Collapse multiple spaces
// ---------------------------------------------------------------------------

fn collapse_spaces(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == ' ' {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            result.push(ch);
            prev_space = false;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Command table (sorted longest-first at compile time)
// ---------------------------------------------------------------------------

/// Commands sorted longest-first to prevent partial matches.
static SORTED_COMMANDS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    let mut cmds: Vec<(&str, &str)> = COMMANDS.to_vec();
    cmds.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    cmds
});

const COMMANDS: &[(&str, &str)] = &[
    // Operators
    (r"\cdot", "dot"),
    (r"\times", "times"),
    (r"\div", "div"),
    (r"\pm", "plus.minus"),
    (r"\mp", "minus.plus"),
    (r"\circ", "compose"),
    (r"\ast", "ast"),
    (r"\star", "star"),
    (r"\oplus", "plus.circle"),
    (r"\otimes", "times.circle"),
    // Relations
    (r"\leq", "<="),
    (r"\geq", ">="),
    (r"\neq", "!="),
    (r"\approx", "approx"),
    (r"\equiv", "equiv"),
    (r"\sim", "tilde.op"),
    (r"\propto", "prop"),
    (r"\ll", "<<"),
    (r"\gg", ">>"),
    (r"\subset", "subset"),
    (r"\supset", "supset"),
    (r"\subseteq", "subset.eq"),
    (r"\supseteq", "supset.eq"),
    (r"\in", "in"),
    (r"\notin", "in.not"),
    (r"\ni", "in.rev"),
    // Escaped characters
    (r"\%", "%"),
    // Arrows
    (r"\implies", "=>"),
    (r"\iff", "<=>"),
    (r"\to", "->"),
    (r"\rightarrow", "->"),
    (r"\leftarrow", "<-"),
    (r"\leftrightarrow", "<->"),
    (r"\Rightarrow", "=>"),
    (r"\Leftarrow", "arrow.l.double"),
    (r"\Leftrightarrow", "<=>"),
    (r"\mapsto", "|->"),
    (r"\uparrow", "arrow.t"),
    (r"\downarrow", "arrow.b"),
    // Big operators
    (r"\int", "integral"),
    (r"\iint", "integral.double"),
    (r"\iiint", "integral.triple"),
    (r"\oint", "integral.cont"),
    (r"\sum", "sum"),
    (r"\prod", "product"),
    (r"\coprod", "product.co"),
    (r"\bigcup", "union.big"),
    (r"\bigcap", "sect.big"),
    // Functions
    (r"\lim", "lim"),
    (r"\sup", "sup"),
    (r"\inf", "inf"),
    (r"\min", "min"),
    (r"\max", "max"),
    (r"\log", "log"),
    (r"\ln", "ln"),
    (r"\exp", "exp"),
    (r"\sin", "sin"),
    (r"\cos", "cos"),
    (r"\tan", "tan"),
    (r"\sec", "sec"),
    (r"\csc", "csc"),
    (r"\cot", "cot"),
    (r"\arcsin", "arcsin"),
    (r"\arccos", "arccos"),
    (r"\arctan", "arctan"),
    (r"\sinh", "sinh"),
    (r"\cosh", "cosh"),
    (r"\tanh", "tanh"),
    (r"\det", "det"),
    (r"\dim", "dim"),
    (r"\ker", "ker"),
    (r"\deg", "deg"),
    (r"\arg", "arg"),
    (r"\gcd", "gcd"),
    (r"\mod", "mod"),
    // Currency
    (r"\pounds", "£"),
    (r"\euro", "€"),
    (r"\yen", "¥"),
    // Misc symbols
    (r"\infty", "infinity"),
    (r"\partial", "diff"),
    (r"\nabla", "nabla"),
    (r"\forall", "forall"),
    (r"\exists", "exists"),
    (r"\neg", "not"),
    (r"\land", "and"),
    (r"\lor", "or"),
    (r"\cup", "union"),
    (r"\cap", "sect"),
    (r"\emptyset", "nothing"),
    (r"\varnothing", "nothing"),
    (r"\ldots", "dots"),
    (r"\cdots", "dots.c"),
    (r"\vdots", "dots.v"),
    (r"\ddots", "dots.down"),
    (r"\dots", "dots"),
    (r"\ell", "ell"),
    (r"\hbar", "planck.reduce"),
    (r"\Re", "Re"),
    (r"\Im", "Im"),
    (r"\aleph", "aleph"),
    // Delimiters
    (r"\left(", "("),
    (r"\right)", ")"),
    (r"\left[", "["),
    (r"\right]", "]"),
    (r"\left\{", "{"),
    (r"\right\}", "}"),
    (r"\left|", "|"),
    (r"\right|", "|"),
    (r"\left.", ""),
    (r"\right.", ""),
    (r"\langle", "angle.l"),
    (r"\rangle", "angle.r"),
    (r"\lfloor", "floor.l"),
    (r"\rfloor", "floor.r"),
    (r"\lceil", "ceil.l"),
    (r"\rceil", "ceil.r"),
    (r"\{", "{"),
    (r"\}", "}"),
    (r"\|", "||"),
    // Accents (bare, no braces)
    (r"\overline", "overline"),
    (r"\underline", "underline"),
    // Spacing
    (r"\quad", "quad"),
    (r"\qquad", "wide"),
    (r"\,", "thin"),
    (r"\;", "med"),
    (r"\!", ""),
    // Greek lowercase
    (r"\alpha", "alpha"),
    (r"\beta", "beta"),
    (r"\gamma", "gamma"),
    (r"\delta", "delta"),
    (r"\epsilon", "epsilon"),
    (r"\varepsilon", "epsilon.alt"),
    (r"\zeta", "zeta"),
    (r"\eta", "eta"),
    (r"\theta", "theta"),
    (r"\vartheta", "theta.alt"),
    (r"\iota", "iota"),
    (r"\kappa", "kappa"),
    (r"\lambda", "lambda"),
    (r"\mu", "mu"),
    (r"\nu", "nu"),
    (r"\xi", "xi"),
    (r"\pi", "pi"),
    (r"\varpi", "pi.alt"),
    (r"\rho", "rho"),
    (r"\varrho", "rho.alt"),
    (r"\sigma", "sigma"),
    (r"\varsigma", "sigma.alt"),
    (r"\tau", "tau"),
    (r"\upsilon", "upsilon"),
    (r"\phi", "phi.alt"),
    (r"\varphi", "phi"),
    (r"\chi", "chi"),
    (r"\psi", "psi"),
    (r"\omega", "omega"),
    // Greek uppercase
    (r"\Gamma", "Gamma"),
    (r"\Delta", "Delta"),
    (r"\Theta", "Theta"),
    (r"\Lambda", "Lambda"),
    (r"\Xi", "Xi"),
    (r"\Pi", "Pi"),
    (r"\Sigma", "Sigma"),
    (r"\Upsilon", "Upsilon"),
    (r"\Phi", "Phi"),
    (r"\Psi", "Psi"),
    (r"\Omega", "Omega"),
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Fractions and roots ---

    #[test]
    fn test_frac() {
        assert_eq!(latex_to_typst(r"\frac{a}{b}"), "frac(a, b)");
        assert_eq!(latex_to_typst(r"\frac{1}{2}"), "frac(1, 2)");
        assert_eq!(latex_to_typst(r"\dfrac{x}{y}"), "frac(x, y)");
        assert_eq!(latex_to_typst(r"\tfrac{a}{b}"), "frac(a, b)");
        assert_eq!(
            latex_to_typst(r"\frac{1}{\sqrt{2}}"),
            "frac(1, sqrt(2))"
        );
    }

    #[test]
    fn test_sqrt() {
        assert_eq!(latex_to_typst(r"\sqrt{x}"), "sqrt(x)");
        assert_eq!(latex_to_typst(r"\sqrt[3]{x}"), "root(3, x)");
        assert_eq!(latex_to_typst(r"\sqrt{a^2 + b^2}"), "sqrt(a^2 + b^2)");
    }

    // --- Greek letters ---

    #[test]
    fn test_greek() {
        assert_eq!(latex_to_typst(r"\alpha"), "alpha");
        assert_eq!(latex_to_typst(r"\beta"), "beta");
        assert_eq!(latex_to_typst(r"\Omega"), "Omega");
    }

    // --- Operators and relations ---

    #[test]
    fn test_operators() {
        assert_eq!(latex_to_typst(r"\leq"), "<=");
        assert_eq!(latex_to_typst(r"\geq"), ">=");
        assert_eq!(latex_to_typst(r"\neq"), "!=");
        assert_eq!(latex_to_typst(r"\approx"), "approx");
        assert_eq!(latex_to_typst(r"\subset"), "subset");
        assert_eq!(latex_to_typst(r"\subseteq"), "subset.eq");
        assert_eq!(latex_to_typst(r"\in"), "in");
        assert_eq!(latex_to_typst(r"\notin"), "in.not");
        assert_eq!(latex_to_typst(r"\cdot"), "dot");
        assert_eq!(latex_to_typst(r"\times"), "times");
        assert_eq!(latex_to_typst(r"\pm"), "plus.minus");
    }

    // --- Arrows ---

    #[test]
    fn test_arrows() {
        assert_eq!(latex_to_typst(r"\to"), "->");
        assert_eq!(latex_to_typst(r"\rightarrow"), "->");
        assert_eq!(latex_to_typst(r"\leftarrow"), "<-");
        assert_eq!(latex_to_typst(r"\implies"), "=>");
        assert_eq!(latex_to_typst(r"\iff"), "<=>");
        assert_eq!(latex_to_typst(r"\mapsto"), "|->");
    }

    // --- Big operators ---

    #[test]
    fn test_big_operators() {
        assert_eq!(latex_to_typst(r"\sum"), "sum");
        assert_eq!(latex_to_typst(r"\prod"), "product");
        assert_eq!(latex_to_typst(r"\int"), "integral");
        assert_eq!(latex_to_typst(r"\iint"), "integral.double");
        assert_eq!(latex_to_typst(r"\oint"), "integral.cont");
    }

    // --- Scripts ---

    #[test]
    fn test_scripts() {
        assert_eq!(latex_to_typst("x^2"), "x^2");
        assert_eq!(latex_to_typst("x_{12}"), "x_(12)");
        assert_eq!(latex_to_typst("x^{2n}"), "x^(2n)");
        assert_eq!(latex_to_typst("a_{i,j}^{k+1}"), "a_(i,j)^(k+1)");
        assert_eq!(latex_to_typst("x_i^2"), "x_i^2");
    }

    // --- Accents ---

    #[test]
    fn test_accents() {
        assert_eq!(latex_to_typst(r"\hat{x}"), "hat(x)");
        assert_eq!(latex_to_typst(r"\bar{y}"), "macron(y)");
        assert_eq!(latex_to_typst(r"\vec{v}"), "arrow(v)");
        assert_eq!(latex_to_typst(r"\dot{x}"), "dot(x)");
        assert_eq!(latex_to_typst(r"\ddot{x}"), "diaer(x)");
        assert_eq!(latex_to_typst(r"\tilde{n}"), "tilde(n)");
    }

    // --- Font commands ---

    #[test]
    fn test_font_commands() {
        assert_eq!(latex_to_typst(r"\mathbf{F}"), "bold(F)");
        assert_eq!(latex_to_typst(r"\mathrm{d}"), "upright(d)");
        assert_eq!(latex_to_typst(r"\mathit{x}"), "italic(x)");
        assert_eq!(latex_to_typst(r"\mathcal{L}"), "cal(L)");
        assert_eq!(latex_to_typst(r#"\text{if }"#), r#""if ""#);
    }

    // --- Blackboard bold ---

    #[test]
    fn test_mathbb() {
        assert_eq!(latex_to_typst(r"\mathbb{R}"), "RR");
        assert_eq!(latex_to_typst(r"\mathbb{N}"), "NN");
        assert_eq!(latex_to_typst(r"\mathbb{Z}"), "ZZ");
    }

    // --- Environments ---

    #[test]
    fn test_aligned() {
        let latex = r"\begin{aligned} f(x) &= x^2 \\ &= x \cdot x \end{aligned}";
        let result = latex_to_typst(latex);
        assert!(result.contains("f(x) = x^2"));
        assert!(result.contains('\\'));
    }

    #[test]
    fn test_cases() {
        let latex = r"\begin{cases} x^2 & \text{if } x > 0 \\ 0 & \text{otherwise} \end{cases}";
        let result = latex_to_typst(latex);
        assert!(result.contains("cases("));
        assert!(result.contains("\"if \""));
        assert!(result.contains("\"otherwise\""));
    }

    #[test]
    fn test_bmatrix() {
        let latex = r"\begin{bmatrix} a & b \\ c & d \end{bmatrix}";
        assert_eq!(latex_to_typst(latex), "mat(delim: \"[\", a, b; c, d)");
    }

    #[test]
    fn test_pmatrix() {
        let latex = r"\begin{pmatrix} 1 & 0 \\ 0 & 1 \end{pmatrix}";
        assert_eq!(latex_to_typst(latex), "mat(delim: \"(\", 1, 0; 0, 1)");
    }

    // --- Spacing ---

    #[test]
    fn test_spacing() {
        assert_eq!(latex_to_typst(r"\quad"), "quad");
        assert_eq!(latex_to_typst(r"\qquad"), "wide");
    }

    // --- Identifier merging prevention ---

    #[test]
    fn test_no_merge_i_pi() {
        assert_eq!(latex_to_typst(r"i\pi"), "i pi");
    }

    #[test]
    fn test_no_merge_m_mathbf() {
        assert_eq!(latex_to_typst(r"m\mathbf{a}"), "m bold(a)");
    }

    #[test]
    fn test_thin_space_dx() {
        let result = latex_to_typst(r"\,dx");
        assert!(result.contains("thin"));
        assert!(result.contains("\"dx\""));
    }

    #[test]
    fn test_subseteq_not_quoted() {
        assert_eq!(latex_to_typst(r"\subseteq"), "subset.eq");
    }

    // --- Complex expressions ---

    #[test]
    fn test_quadratic_formula() {
        let result = latex_to_typst(r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}");
        assert!(result.contains("frac("));
        assert!(result.contains("sqrt("));
        assert!(result.contains("plus.minus"));
    }

    #[test]
    fn test_euler_identity() {
        let result = latex_to_typst(r"e^{i\pi} + 1 = 0");
        assert!(result.contains("e^(i pi)"));
    }

    #[test]
    fn test_sum_with_limits() {
        let result = latex_to_typst(r"\sum_{i=1}^{n} i");
        assert!(result.contains("sum_(i=1)^(n)"));
    }

    // --- Misc symbols ---

    #[test]
    fn test_misc_symbols() {
        assert_eq!(latex_to_typst(r"\infty"), "infinity");
        assert_eq!(latex_to_typst(r"\partial"), "diff");
        assert_eq!(latex_to_typst(r"\nabla"), "nabla");
        assert_eq!(latex_to_typst(r"\forall"), "forall");
        assert_eq!(latex_to_typst(r"\exists"), "exists");
        assert_eq!(latex_to_typst(r"\emptyset"), "nothing");
    }

    // --- Delimiters ---

    #[test]
    fn test_delimiters() {
        assert_eq!(latex_to_typst(r"\left("), "(");
        assert_eq!(latex_to_typst(r"\right)"), ")");
        assert_eq!(latex_to_typst(r"\left["), "[");
        assert_eq!(latex_to_typst(r"\right]"), "]");
        assert_eq!(latex_to_typst(r"\{"), "{");
        assert_eq!(latex_to_typst(r"\}"), "}");
    }
}
