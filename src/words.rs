//! Word-level intra-line diffing.
//!
//! Highlights the characters that actually changed within a modified line.
//! Offsets are emitted in **UTF-16 code units**, not Rust char indices, so the
//! browser can slice strings directly — the two disagree for anything outside
//! the BMP, and an emoji in a line would otherwise shift every highlight.

use crate::model::{Hunk, Line, LineKind, Range};

/// Lines longer than this are left unhighlighted; the pairing is rarely
/// meaningful and the cost is quadratic.
const MAX_LINE: usize = 1000;
/// If more than this fraction of a line changed, it is a rewrite rather than
/// an edit, and highlighting all of it is noise.
const REWRITE_RATIO: f32 = 0.75;

#[derive(PartialEq)]
struct Token {
    text: String,
    /// Offset of this token within the line, in UTF-16 code units.
    at: usize,
    len: usize,
}

/// Split into words and single punctuation characters, the granularity that
/// reads best in a diff.
fn tokenize(s: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut start = 0usize;
    let mut at = 0usize;
    for c in s.chars() {
        let w = c.len_utf16();
        if c.is_alphanumeric() || c == '_' {
            if buf.is_empty() {
                start = at;
            }
            buf.push(c);
        } else {
            if !buf.is_empty() {
                out.push(Token { len: at - start, text: std::mem::take(&mut buf), at: start });
            }
            out.push(Token { text: c.to_string(), at, len: w });
        }
        at += w;
    }
    if !buf.is_empty() {
        out.push(Token { len: at - start, text: buf, at: start });
    }
    out
}

/// Longest common subsequence over tokens. Inputs are pre-trimmed by common
/// prefix and suffix, so the table stays small in practice.
fn lcs_flags(a: &[Token], b: &[Token]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![0u16; (n + 1) * (m + 1)];
    let idx = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[idx(i, j)] = if a[i].text == b[j].text {
                dp[idx(i + 1, j + 1)] + 1
            } else {
                dp[idx(i + 1, j)].max(dp[idx(i, j + 1)])
            };
        }
    }
    let (mut ka, mut kb) = (vec![true; n], vec![true; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i].text == b[j].text {
            ka[i] = false;
            kb[j] = false;
            i += 1;
            j += 1;
        } else if dp[idx(i + 1, j)] >= dp[idx(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (ka, kb)
}

fn merge(mut ranges: Vec<Range>) -> Vec<Range> {
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            // Adjacent runs are joined so a changed word and the punctuation
            // beside it render as one highlight rather than a dotted line.
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

fn changed_ranges(old: &str, new: &str) -> Option<(Vec<Range>, Vec<Range>)> {
    if old.len() > MAX_LINE || new.len() > MAX_LINE || old == new {
        return None;
    }
    let (ta, tb) = (tokenize(old), tokenize(new));

    // Trim the shared head and tail before running the quadratic step.
    let head = ta
        .iter()
        .zip(tb.iter())
        .take_while(|(x, y)| x.text == y.text)
        .count();
    let tail = ta[head..]
        .iter()
        .rev()
        .zip(tb[head..].iter().rev())
        .take_while(|(x, y)| x.text == y.text)
        .count();
    let (ma, mb) = (&ta[head..ta.len() - tail], &tb[head..tb.len() - tail]);

    let (fa, fb) = lcs_flags(ma, mb);
    let ra: Vec<Range> = ma
        .iter()
        .zip(&fa)
        .filter(|(_, c)| **c)
        .map(|(t, _)| Range { start: t.at, end: t.at + t.len })
        .collect();
    let rb: Vec<Range> = mb
        .iter()
        .zip(&fb)
        .filter(|(_, c)| **c)
        .map(|(t, _)| Range { start: t.at, end: t.at + t.len })
        .collect();

    // A near-total rewrite is better shown as a plain replacement.
    let span = |rs: &[Range]| rs.iter().map(|r| r.end - r.start).sum::<usize>() as f32;
    let (wa, wb) = (old.chars().count() as f32, new.chars().count() as f32);
    if wa > 0.0 && wb > 0.0 && span(&ra) / wa > REWRITE_RATIO && span(&rb) / wb > REWRITE_RATIO {
        return None;
    }
    if ra.is_empty() && rb.is_empty() {
        return None;
    }
    Some((merge(ra), merge(rb)))
}

/// Pair deletions with the additions that replaced them and annotate both.
/// Only equal-length runs are paired: guessing across a 3-for-1 replacement
/// produces highlights that mislead more than they help.
pub fn annotate(hunks: &mut [Hunk]) {
    for hunk in hunks {
        let lines: &mut Vec<Line> = &mut hunk.lines;
        let mut i = 0;
        while i < lines.len() {
            if lines[i].kind != LineKind::Del {
                i += 1;
                continue;
            }
            let dels = i + lines[i..]
                .iter()
                .take_while(|l| l.kind == LineKind::Del)
                .count();
            let adds = dels + lines[dels..]
                .iter()
                .take_while(|l| l.kind == LineKind::Add)
                .count();
            let (dn, an) = (dels - i, adds - dels);
            if dn == an && dn > 0 {
                for k in 0..dn {
                    let old = lines[i + k].content.clone();
                    let new = lines[dels + k].content.clone();
                    if let Some((ra, rb)) = changed_ranges(&old, &new) {
                        lines[i + k].words = Some(ra);
                        lines[dels + k].words = Some(rb);
                    }
                }
            }
            i = adds.max(i + 1);
        }
    }
}
