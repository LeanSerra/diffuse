//! Syntax highlighting.
//!
//! Highlighting is computed over the **whole file**, never over the diff's
//! fragments. A hunk can begin inside a block comment or a multi-line string,
//! and a per-line highlighter would render that opening as live code —
//! confidently wrong, which is worse than leaving it plain.
//!
//! Scopes collapse to a handful of classes rather than colours, so the palette
//! stays in CSS and follows the light and dark themes like everything else.
//! Offsets are UTF-16 code units, matching the word-level ranges, so the
//! browser can slice with them directly.

use std::sync::OnceLock;

use syntect::easy::ScopeRegionIterator;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Files past this many lines are left plain; they already sit behind a click.
pub const MAX_LINES: usize = 5000;

pub use crate::model::Span;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// Scopes checked innermost first, so the most specific one wins.
///
/// An empty class means "leave it plain": operators and punctuation are
/// deliberately uncoloured, which is what editors and GitHub do. They still
/// need rules, or `keyword.operator` would fall through to `keyword` and paint
/// every `=` and `+` like a keyword.
fn rules() -> &'static [(Scope, &'static str)] {
    static RULES: OnceLock<Vec<(Scope, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        [
            ("keyword.operator", ""),
            ("punctuation", ""),
            ("constant.numeric", "num"),
            ("constant.language", "const"),
            ("constant.other", "const"),
            ("variable.other.constant", "const"),
            ("entity.name.constant", "const"),
            ("entity.name.function", "fn"),
            ("entity.name.macro", "fn"),
            ("support.function", "fn"),
            ("variable.function", "fn"),
            ("entity.name.type", "typ"),
            ("entity.name.class", "typ"),
            ("entity.name.struct", "typ"),
            ("entity.name.enum", "typ"),
            ("entity.name.union", "typ"),
            ("entity.name.trait", "typ"),
            ("entity.name.namespace", "typ"),
            ("support.type", "typ"),
            ("support.class", "typ"),
            ("entity.name.tag", "tag"),
            ("markup.heading", "kw"),
            ("markup.bold", "kw"),
            ("markup.italic", "typ"),
            ("markup.raw", "str"),
            ("markup.underline.link", "attr"),
            ("markup.list", ""),
            ("entity.other.attribute-name", "attr"),
            ("variable.parameter", ""),
            ("storage", "kw"),
            ("keyword", "kw"),
            ("variable.language", "kw"),
        ]
        .into_iter()
        .filter_map(|(s, c)| Scope::new(s).ok().map(|s| (s, c)))
        .collect()
    })
}

fn classify(stack: &ScopeStack) -> Option<&'static str> {
    static CONTAINERS: OnceLock<(Scope, Scope, Scope)> = OnceLock::new();
    let (comment, string, escape) = CONTAINERS.get_or_init(|| {
        (
            Scope::new("comment").unwrap(),
            Scope::new("string").unwrap(),
            Scope::new("constant.character.escape").unwrap(),
        )
    });

    let mut in_comment = false;
    let mut in_string = false;
    for scope in stack.as_slice() {
        if escape.is_prefix_of(*scope) {
            return Some("esc");
        }
        in_comment |= comment.is_prefix_of(*scope);
        in_string |= string.is_prefix_of(*scope);
    }
    if in_comment {
        return Some("com");
    }
    if in_string {
        return Some("str");
    }

    for scope in stack.as_slice().iter().rev() {
        for (prefix, class) in rules() {
            if prefix.is_prefix_of(*scope) {
                return Some(class);
            }
        }
    }
    None
}

/// Per-line spans for `text`, or `None` when the file has no grammar, is too
/// long, or looks binary.
pub fn highlight(path: &str, text: &str) -> Option<Vec<Vec<Span>>> {
    if text.is_empty() || text.len() > 4 * 1024 * 1024 || text.contains('\0') {
        return None;
    }
    let set = syntaxes();
    let name = path.rsplit('/').next().unwrap_or(path);
    let syntax = name
        .rsplit_once('.')
        .and_then(|(_, ext)| set.find_syntax_by_extension(ext))
        .or_else(|| set.find_syntax_by_extension(name))
        .or_else(|| set.find_syntax_by_first_line(text.lines().next().unwrap_or("")))?;

    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out: Vec<Vec<Span>> = Vec::new();

    for line in LinesWithEndings::from(text) {
        if out.len() >= MAX_LINES {
            return None;
        }
        let Ok(ops) = state.parse_line(line, set) else {
            return None;
        };
        let mut spans: Vec<Span> = Vec::new();
        let mut at = 0usize; // UTF-16 offset within the line
        for (piece, op) in ScopeRegionIterator::new(&ops, line) {
            if stack.apply(op).is_err() {
                return None;
            }
            if piece.is_empty() {
                continue;
            }
            // The trailing newline is not part of the rendered line.
            let visible = piece.trim_end_matches(['\n', '\r']);
            let width = visible.encode_utf16().count();
            if width > 0 {
                if let Some(class) = classify(&stack).filter(|c| !c.is_empty()) {
                    match spans.last_mut() {
                        // Adjacent pieces of the same class are one span.
                        Some(last) if last.class == class && last.end == at => {
                            last.end = at + width;
                        }
                        _ => spans.push(Span {
                            start: at,
                            end: at + width,
                            class,
                        }),
                    }
                }
            }
            at += piece.encode_utf16().count();
        }
        out.push(spans);
    }
    Some(out)
}

/// Attach syntax classes to a parsed file's lines.
///
/// Added and context lines are looked up in the new text, removals in the old
/// text, so both sides of a change are coloured from a file that actually
/// parses. Combined diffs are skipped: their removals belong to different
/// parents, so there is no single old file to read them from.
pub fn annotate(file: &mut crate::model::FileDiff, old_text: Option<&str>, new_text: Option<&str>) {
    use crate::model::LineKind;

    if file.binary || file.combined {
        return;
    }
    let new_lines = new_text.and_then(|t| highlight(&file.path, t));
    let old_name = file.old_path.as_deref().unwrap_or(&file.path);
    let old_lines = old_text.and_then(|t| highlight(old_name, t));
    if new_lines.is_none() && old_lines.is_none() {
        return;
    }

    for hunk in file.hunks.iter_mut() {
        for line in hunk.lines.iter_mut() {
            let spans = match line.kind {
                LineKind::Del => old_lines
                    .as_ref()
                    .zip(line.old_no)
                    .and_then(|(all, n)| all.get(n as usize - 1)),
                _ => new_lines
                    .as_ref()
                    .zip(line.new_no)
                    .and_then(|(all, n)| all.get(n as usize - 1)),
            };
            if let Some(spans) = spans {
                if !spans.is_empty() {
                    line.syntax = Some(spans.clone());
                }
            }
        }
    }
}
