//! Unified-diff parser.
//!
//! Hand-rolled rather than taken from a crate: every candidate on crates.io is
//! 0.x, and this is the core correctness of the product. Stats are *not*
//! derived here — they come from `git diff --numstat -z`, which is
//! authoritative. This module only produces renderable structure.

use crate::model::{FileDiff, Hunk, Line, LineKind};

/// Decode git's C-style path quoting. Even with `core.quotepath=false`, paths
/// containing `"`, `\` or control characters arrive quoted.
pub fn unquote_path(s: &str) -> String {
    if !s.starts_with('"') || !s.ends_with('"') || s.len() < 2 {
        return s.to_string();
    }
    let inner = &s[1..s.len() - 1];
    let mut bytes = Vec::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some('n') => bytes.push(b'\n'),
            Some('t') => bytes.push(b'\t'),
            Some('r') => bytes.push(b'\r'),
            Some('a') => bytes.push(0x07),
            Some('b') => bytes.push(0x08),
            Some('f') => bytes.push(0x0c),
            Some('v') => bytes.push(0x0b),
            Some('"') => bytes.push(b'"'),
            Some('\\') => bytes.push(b'\\'),
            // `\NNN` octal escapes encode raw bytes, which may combine into
            // one multi-byte UTF-8 character.
            Some(d) if d.is_digit(8) => {
                let mut v = d.to_digit(8).unwrap();
                for _ in 0..2 {
                    let Some(n) = chars.clone().next() else { break };
                    if let Some(dv) = n.to_digit(8) {
                        v = v * 8 + dv;
                        chars.next();
                    } else {
                        break;
                    }
                }
                bytes.push(v as u8);
            }
            Some(other) => {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
            None => break,
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn strip_prefix(path: &str) -> String {
    let p = unquote_path(path);
    match p.split_once('/') {
        Some((_, rest)) if p.starts_with("a/") || p.starts_with("b/") => rest.to_string(),
        _ => p,
    }
}

/// Split `a/foo b/foo` from a `diff --git` line. Paths may contain spaces, so
/// the split point is found by looking for a ` b/` (or ` "b/`) that leaves a
/// well-formed left half — the same ambiguity git itself resolves with the
/// `---`/`+++` lines, which we prefer when they exist.
fn split_git_header(rest: &str) -> Option<(String, String)> {
    if let Some(stripped) = rest.strip_prefix('"') {
        // Quoted left path: it ends at the first unescaped quote.
        let mut end = None;
        let bytes = stripped.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 1,
                b'"' => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        let e = end?;
        let left = &rest[..e + 2];
        let right = rest[e + 2..].trim_start();
        return Some((strip_prefix(left), strip_prefix(right)));
    }
    let mut search = 0;
    while let Some(idx) = rest[search..].find(" b/") {
        let at = search + idx;
        let left = &rest[..at];
        let right = &rest[at + 1..];
        if left.starts_with("a/") {
            return Some((strip_prefix(left), strip_prefix(right)));
        }
        search = at + 1;
    }
    // `--no-prefix` style output, or something unexpected: fall back to a
    // midpoint guess so the file still renders.
    let half = rest.len() / 2;
    if rest.as_bytes().get(half) == Some(&b' ') {
        return Some((
            strip_prefix(&rest[..half]),
            strip_prefix(&rest[half + 1..]),
        ));
    }
    None
}

struct HunkHeader {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    section: Option<String>,
    parents: usize,
}

/// `@@ -12,7 +12,9 @@ section` or, for merge commits, `@@@ -1,7 -1,7 +1,9 @@@`.
fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    let at_count = line.chars().take_while(|c| *c == '@').count();
    if at_count < 2 {
        return None;
    }
    let closing = format!(" {} ", "@".repeat(at_count));
    let (spec, section) = match line[at_count..].find(&closing[1..]) {
        Some(i) => {
            let s = &line[at_count..at_count + i];
            let after = &line[at_count + i + at_count + 1..];
            let sec = after.trim();
            (
                s.trim(),
                if sec.is_empty() {
                    None
                } else {
                    Some(sec.to_string())
                },
            )
        }
        None => (line[at_count..].trim().trim_end_matches('@').trim(), None),
    };

    let mut olds = Vec::new();
    let mut new = (0u32, 1u32);
    for part in spec.split_whitespace() {
        let Some((sign, nums)) = part.split_at_checked(1) else {
            continue;
        };
        let mut it = nums.split(',');
        let start: u32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        // A missing count means exactly one line.
        let count: u32 = it.next().map(|v| v.parse().unwrap_or(0)).unwrap_or(1);
        match sign {
            "-" => olds.push((start, count)),
            "+" => new = (start, count),
            _ => {}
        }
    }
    let first_old = olds.first().copied().unwrap_or((0, 0));
    Some(HunkHeader {
        old_start: first_old.0,
        old_lines: first_old.1,
        new_start: new.0,
        new_lines: new.1,
        section,
        parents: at_count - 1,
    })
}

pub fn parse_patch(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cur: Option<FileDiff> = None;
    let mut hunk: Option<Hunk> = None;
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    let mut parents = 1usize;
    let mut in_binary_payload = false;

    macro_rules! close_hunk {
        () => {
            if let (Some(h), Some(f)) = (hunk.take(), cur.as_mut()) {
                f.hunks.push(h);
            }
        };
    }

    for raw in text.lines() {
        // `diff --git` starts a file; anything before the first one is a
        // commit header from `git show` and is skipped.
        if let Some(rest) = raw.strip_prefix("diff --git ") {
            close_hunk!();
            if let Some(f) = cur.take() {
                files.push(f);
            }
            in_binary_payload = false;
            parents = 1;
            let (old, new) = split_git_header(rest).unwrap_or_default();
            cur = Some(FileDiff {
                path: new,
                old_path: Some(old),
                ..Default::default()
            });
            continue;
        }
        if let Some(rest) = raw
            .strip_prefix("diff --cc ")
            .or_else(|| raw.strip_prefix("diff --combined "))
        {
            close_hunk!();
            if let Some(f) = cur.take() {
                files.push(f);
            }
            in_binary_payload = false;
            let p = unquote_path(rest);
            cur = Some(FileDiff {
                path: p,
                combined: true,
                ..Default::default()
            });
            continue;
        }

        let Some(file) = cur.as_mut() else { continue };

        if let Some(h) = parse_hunk_header(raw) {
            close_hunk!();
            let file = cur.as_mut().unwrap();
            old_no = h.old_start;
            new_no = h.new_start;
            parents = h.parents;
            if h.parents > 1 {
                file.combined = true;
            }
            hunk = Some(Hunk {
                header: raw.to_string(),
                old_start: h.old_start,
                old_lines: h.old_lines,
                new_start: h.new_start,
                new_lines: h.new_lines,
                section: h.section,
                lines: Vec::new(),
            });
            continue;
        }

        if hunk.is_none() {
            // Still in the per-file header block.
            if let Some(m) = raw.strip_prefix("old mode ") {
                file.old_mode = Some(m.trim().to_string());
            } else if let Some(m) = raw.strip_prefix("new mode ") {
                file.new_mode = Some(m.trim().to_string());
            } else if let Some(m) = raw.strip_prefix("new file mode ") {
                file.new_mode = Some(m.trim().to_string());
                file.old_path = None;
            } else if let Some(m) = raw.strip_prefix("deleted file mode ") {
                file.old_mode = Some(m.trim().to_string());
            } else if let Some(p) = raw.strip_prefix("rename from ") {
                file.old_path = Some(unquote_path(p));
            } else if let Some(p) = raw.strip_prefix("rename to ") {
                file.path = unquote_path(p);
            } else if let Some(p) = raw.strip_prefix("copy from ") {
                file.old_path = Some(unquote_path(p));
            } else if let Some(p) = raw.strip_prefix("copy to ") {
                file.path = unquote_path(p);
            } else if raw.starts_with("Binary files ") || raw.starts_with("GIT binary patch") {
                file.binary = true;
                in_binary_payload = true;
            } else if let Some(p) = raw.strip_prefix("--- ") {
                if p != "/dev/null" {
                    file.old_path = Some(strip_prefix(p));
                } else {
                    file.old_path = None;
                }
            } else if let Some(p) = raw.strip_prefix("+++ ") {
                if p != "/dev/null" {
                    file.path = strip_prefix(p);
                }
            }
            continue;
        }
        if in_binary_payload {
            continue;
        }

        let h = hunk.as_mut().unwrap();

        // `\ No newline at end of file` annotates the preceding line.
        if raw.starts_with('\\') {
            if let Some(last) = h.lines.last_mut() {
                last.no_newline = true;
            }
            continue;
        }

        // A combined diff carries one marker column per parent.
        let markers: String = raw.chars().take(parents).collect();
        let content: String = raw.chars().skip(parents).collect();
        let kind = if markers.contains('+') {
            LineKind::Add
        } else if markers.contains('-') {
            LineKind::Del
        } else if markers.chars().all(|c| c == ' ') && !markers.is_empty() {
            LineKind::Context
        } else {
            continue;
        };

        let (mut o, n) = match kind {
            LineKind::Context => {
                let v = (Some(old_no), Some(new_no));
                old_no += 1;
                new_no += 1;
                v
            }
            LineKind::Del => {
                let v = (Some(old_no), None);
                old_no += 1;
                v
            }
            LineKind::Add => {
                let v = (None, Some(new_no));
                new_no += 1;
                v
            }
        };

        // In a combined diff each `-` belongs to one specific parent, so a
        // single old-side counter cannot be correct. Report no old number
        // rather than a wrong one; the result side stays unambiguous.
        if parents > 1 {
            o = None;
        }

        h.lines.push(Line {
            kind,
            old_no: o,
            new_no: n,
            content,
            no_newline: false,
            words: None,
                syntax: None,
        });
    }

    close_hunk!();
    if let Some(f) = cur.take() {
        files.push(f);
    }
    // A rename with no content change still reports old_path; an ordinary
    // modification should not.
    for f in files.iter_mut() {
        if f.old_path.as_deref() == Some(f.path.as_str()) {
            f.old_path = None;
        }
    }
    files
}
