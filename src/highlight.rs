// Syntax highlighting for the editor viewport.
//
// Only Rust (.rs) files are highlighted. All other extensions get no
// highlighting (plain text).  The highlighter runs on one line at a time and
// is designed to be fast enough for files under 10,000 lines.

use crossterm::style::Color;

/// A highlighted span: a run of characters sharing the same colour.
#[derive(Debug, Clone)]
pub struct Span {
    pub text: String,
    pub color: Option<Color>,
}

impl Span {
    fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), color: None }
    }
    fn colored(text: impl Into<String>, color: Color) -> Self {
        Self { text: text.into(), color: Some(color) }
    }
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn",
    "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "union", "unsafe", "use", "where", "while",
];

/// Highlight a single line of text for the given file extension.
///
/// `in_block_comment` indicates whether we entered this line inside a `/* */`
/// block comment.  `theme` is `"dark"` or `"light"` and controls the colour
/// palette.  Returns spans and the updated `in_block_comment` state.
pub fn highlight_line(
    line: &str,
    ext: Option<&str>,
    in_block_comment: bool,
    theme: &str,
) -> (Vec<Span>, bool) {
    if ext != Some("rs") {
        return (vec![Span::plain(line)], in_block_comment);
    }
    highlight_rust(line, in_block_comment, theme)
}

fn push_plain(spans: &mut Vec<Span>, buf: &mut String) {
    if !buf.is_empty() {
        spans.push(Span::plain(std::mem::take(buf)));
    }
}

fn highlight_rust(line: &str, mut in_block: bool, theme: &str) -> (Vec<Span>, bool) {
    let light = theme == "light";
    let kw_color    = if light { Color::DarkBlue }   else { Color::Blue };
    let str_color   = if light { Color::DarkGreen }  else { Color::Green };
    let num_color   = if light { Color::DarkYellow } else { Color::Yellow };
    let type_color  = if light { Color::DarkCyan }   else { Color::Cyan };

    let mut spans: Vec<Span> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut plain: String = String::new();

    while i < len {
        // inside a block comment
        if in_block {
            push_plain(&mut spans, &mut plain);
            let start = i;
            while i < len {
                if i + 1 < len && chars[i] == '*' && chars[i + 1] == '/' {
                    i += 2;
                    in_block = false;
                    break;
                }
                i += 1;
            }
            spans.push(Span::colored(chars[start..i].iter().collect::<String>(), Color::DarkGrey));
            continue;
        }

        // line comment
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            push_plain(&mut spans, &mut plain);
            spans.push(Span::colored(chars[i..].iter().collect::<String>(), Color::DarkGrey));
            return (spans, false);
        }

        // block comment start
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
            push_plain(&mut spans, &mut plain);
            in_block = true;
            let start = i;
            i += 2;
            while i < len {
                if i + 1 < len && chars[i] == '*' && chars[i + 1] == '/' {
                    i += 2;
                    in_block = false;
                    break;
                }
                i += 1;
            }
            spans.push(Span::colored(chars[start..i].iter().collect::<String>(), Color::DarkGrey));
            continue;
        }

        // string literal
        if chars[i] == '"' {
            push_plain(&mut spans, &mut plain);
            let start = i;
            i += 1;
            while i < len {
                if chars[i] == '\\' { i += 2; }
                else if chars[i] == '"' { i += 1; break; }
                else { i += 1; }
            }
            spans.push(Span::colored(chars[start..i].iter().collect::<String>(), str_color));
            continue;
        }

        // char literal
        if chars[i] == '\'' {
            // Distinguish lifetime annotations (`'a `) from char literals (`'a'`).
            let is_lifetime = i + 1 < len && chars[i + 1].is_alphabetic()
                && chars[i + 1..].iter().position(|&c| c == '\'').map(|p| p > 2).unwrap_or(true);
            if !is_lifetime {
                push_plain(&mut spans, &mut plain);
                let start = i;
                i += 1;
                while i < len {
                    if chars[i] == '\\' { i += 2; }
                    else if chars[i] == '\'' { i += 1; break; }
                    else { i += 1; }
                }
                spans.push(Span::colored(chars[start..i].iter().collect::<String>(), str_color));
                continue;
            }
        }

        // number literal
        if chars[i].is_ascii_digit() {
            push_plain(&mut spans, &mut plain);
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            spans.push(Span::colored(chars[start..i].iter().collect::<String>(), num_color));
            continue;
        }

        // identifier: keyword or capitalised type
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            push_plain(&mut spans, &mut plain);
            if KEYWORDS.contains(&word.as_str()) {
                spans.push(Span::colored(word, kw_color));
            } else if word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                spans.push(Span::colored(word, type_color));
            } else {
                spans.push(Span::plain(word));
            }
            continue;
        }

        // plain character
        plain.push(chars[i]);
        i += 1;
    }

    push_plain(&mut spans, &mut plain);
    (spans, in_block)
}
