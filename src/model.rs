//! Wire types. The client never sees raw diff text.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    Untracked,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: Status,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
    pub untracked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Context,
    Add,
    Del,
}

/// A half-open `[start, end)` range of char indices within a line's content,
/// marking the characters that actually changed.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

/// A run of characters sharing one syntax class. Offsets are UTF-16 code
/// units, like `Range`, so the browser can slice with them directly.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub class: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub kind: LineKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_no: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_no: Option<u32>,
    pub content: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_newline: bool,
    /// Character ranges that differ from the paired line, when word-level
    /// diffing found a confident pairing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<Range>>,
    /// Syntax classes for this line, from highlighting the whole file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syntax: Option<Vec<Span>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The `@@ ... @@ <section>` trailer git emits, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FileDiff {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
    /// A combined diff from a merge commit: two columns of markers, rendered
    /// in a degraded read-only form.
    pub combined: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_mode: Option<String>,
    /// Set when the file exceeded the render ceiling and was not sent.
    pub truncated: bool,
}
