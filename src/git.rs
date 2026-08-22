//! The git invocation layer.
//!
//! diffuse shells out to git rather than using a library because the CLI
//! contract requires git's full revision syntax (`main...HEAD`, `HEAD@{2}`,
//! `:/fix typo`). git is the parser.
//!
//! diffuse holds no diff state: the file list comes from one `--numstat -z`
//! call and each file's body is fetched by re-running the same argv with a
//! pathspec appended.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::cli::{Invocation, Subcommand};
use crate::model::{FileEntry, Status};

#[derive(Debug)]
pub struct GitError {
    pub message: String,
    pub stderr: String,
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.stderr.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}", self.stderr.trim_end())
        }
    }
}

impl From<io::Error> for GitError {
    fn from(e: io::Error) -> Self {
        GitError {
            message: format!("could not run git: {e}"),
            stderr: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Repo {
    pub root: PathBuf,
}

/// Locate the repository containing `cwd`, exactly as git would.
pub fn discover(cwd: &Path) -> Result<Repo, GitError> {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !out.status.success() {
        return Err(GitError {
            message: "not a git repository".into(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    let root = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    Ok(Repo {
        root: PathBuf::from(root),
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Head {
    pub branch: String,
    pub sha: String,
    pub subject: String,
    pub detached: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommitMeta {
    pub sha: String,
    pub author: String,
    pub date: String,
    pub subject: String,
    pub body: String,
    /// A merge is shown against its first parent, which the UI says out loud.
    pub merge: bool,
}

pub struct Runner {
    pub repo: Repo,
    pub inv: Invocation,
}

impl Runner {
    pub fn new(repo: Repo, inv: Invocation) -> Self {
        Runner { repo, inv }
    }

    fn raw(&self, args: &[String]) -> Result<Output, GitError> {
        Ok(Command::new("git")
            .current_dir(&self.repo.root)
            .args(args)
            .output()?)
    }

    fn plumbing(&self, args: &[&str]) -> Option<String> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let out = self.raw(&owned).ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    }

    /// Options that protect the parser and the transport. Never anything that
    /// changes what the diff means: colour escapes appear even through a pipe
    /// when `color.ui = always`, an external diff driver replaces the patch
    /// with arbitrary output, and non-ASCII paths arrive octal-escaped.
    fn base(&self) -> Vec<String> {
        let mut v = vec![
            "-c".into(),
            "core.quotepath=false".into(),
            self.inv.subcommand.as_str().into(),
            "--no-color".into(),
            "--no-ext-diff".into(),
        ];
        if self.inv.subcommand == Subcommand::Show {
            // Suppress git's commit header; diffuse renders that itself from
            // `commit_meta`, so the patch stream starts at `diff --git`.
            v.push("--format=".into());
            // `git show <merge>` prints no patch at all, yet still reports
            // stats — against the first parent. Without this the sidebar shows
            // line counts while the body claims nothing changed. This asks for
            // exactly the diff the stats already describe. It has no effect on
            // ordinary commits, and a `--cc` or `-m` of the user's own comes
            // later on the command line and still wins.
            v.push("--diff-merges=first-parent".into());
        }
        v
    }

    /// Split the user's arguments at `--` into (revisions and flags, pathspecs).
    fn split_pathspec(&self) -> (Vec<String>, Vec<String>) {
        match self.inv.args.iter().position(|a| a == "--") {
            Some(i) => (
                self.inv.args[..i].to_vec(),
                self.inv.args[i + 1..].to_vec(),
            ),
            None => (self.inv.args.clone(), Vec::new()),
        }
    }

    /// Run the user's command as given, once, to check it is valid. Mirrors
    /// git's own failure behaviour so the terminal reports the problem.
    pub fn validate(&self) -> Result<(), GitError> {
        let mut args = self.base();
        args.push("--numstat".into());
        args.push("-z".into());
        args.extend(self.inv.args.iter().cloned());
        let out = self.raw(&args)?;
        if !out.status.success() {
            return Err(GitError {
                message: "git command failed".into(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        Ok(())
    }

    /// The file list, from git's authoritative machine-readable stat output.
    /// Counting lines out of the parsed patch would be a second source of truth.
    pub fn file_list(&self) -> Result<Vec<FileEntry>, GitError> {
        let mut args = self.base();
        args.push("--numstat".into());
        args.push("-z".into());
        args.extend(self.inv.args.iter().cloned());
        let out = self.raw(&args)?;
        if !out.status.success() {
            return Err(GitError {
                message: "git command failed".into(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        let mut files = parse_numstat_z(&String::from_utf8_lossy(&out.stdout));
        self.classify(&mut files);

        if self.inv.want_untracked && self.worktree_is_right_side() {
            files.extend(self.untracked()?);
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// numstat reports counts but not add/delete/modify. `--name-status` does,
    /// and costs one more cheap call rather than a second parse of the patch.
    fn classify(&self, files: &mut [FileEntry]) {
        let mut args = self.base();
        args.push("--name-status".into());
        args.push("-z".into());
        args.extend(self.inv.args.iter().cloned());
        let Ok(out) = self.raw(&args) else { return };
        if !out.status.success() {
            return;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut fields = text.split('\0').filter(|s| !s.is_empty());
        while let Some(code) = fields.next() {
            let letter = code.chars().next().unwrap_or('M');
            // Renames and copies are followed by two paths, not one.
            let (status, path) = match letter {
                'R' | 'C' => {
                    let _old = fields.next();
                    let Some(new) = fields.next() else { break };
                    let s = if letter == 'R' {
                        Status::Renamed
                    } else {
                        Status::Copied
                    };
                    (s, new.to_string())
                }
                other => {
                    let Some(p) = fields.next() else { break };
                    let s = match other {
                        'A' => Status::Added,
                        'D' => Status::Deleted,
                        _ => Status::Modified,
                    };
                    (s, p.to_string())
                }
            };
            if let Some(f) = files.iter_mut().find(|f| f.path == path) {
                f.status = status;
            }
        }
    }

    /// Untracked files are invisible to every git diff mode, so a brand-new
    /// file would silently not appear. They are included only when the right
    /// side of the comparison is the working tree.
    fn untracked(&self) -> Result<Vec<FileEntry>, GitError> {
        let mut args: Vec<String> = ["ls-files", "--others", "--exclude-standard", "-z"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // The user's pathspecs bound the untracked listing too, or a
        // `diffuse HEAD -- src/` would leak new files from outside src/.
        let (_, pathspecs) = self.split_pathspec();
        if !pathspecs.is_empty() {
            args.push("--".into());
            args.extend(pathspecs);
        }
        let out = self.raw(&args)?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut files = Vec::new();
        for path in text.split('\0').filter(|s| !s.is_empty()) {
            let full = self.repo.root.join(path);
            let Some((adds, binary)) = measure_untracked(&full) else {
                continue;
            };
            files.push(FileEntry {
                path: path.to_string(),
                old_path: None,
                status: Status::Untracked,
                additions: adds,
                deletions: 0,
                binary,
                untracked: true,
            });
        }
        Ok(files)
    }

    /// The patch for a single file: the same argv, with the pathspec replaced
    /// by this file. Replacing rather than appending matters — appending would
    /// union with the user's pathspecs and return the whole diff again.
    pub fn patch_for(&self, path: &str, old_path: Option<&str>) -> Result<String, GitError> {
        let (head, _) = self.split_pathspec();
        let mut args: Vec<String> = vec!["--literal-pathspecs".into()];
        args.extend(self.base());
        args.extend(head);
        args.push("--".into());
        // A rename needs both sides present or git reports it as an add.
        if let Some(old) = old_path {
            args.push(old.to_string());
        }
        args.push(path.to_string());
        let out = self.raw(&args)?;
        if !out.status.success() {
            return Err(GitError {
                message: format!("could not diff {path}"),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        // Diff content is not guaranteed to be UTF-8; render what we can
        // rather than failing the whole file.
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn read_untracked(&self, path: &str) -> Result<String, GitError> {
        let full = self.repo.root.join(path);
        let bytes = std::fs::read(&full)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn head(&self) -> Option<Head> {
        let sha = self.plumbing(&["rev-parse", "--short", "HEAD"])?;
        let branch = self
            .plumbing(&["symbolic-ref", "--short", "HEAD"])
            .unwrap_or_default();
        let subject = self
            .plumbing(&["log", "-1", "--format=%s"])
            .unwrap_or_default();
        Some(Head {
            detached: branch.is_empty(),
            branch: if branch.is_empty() {
                format!("detached at {sha}")
            } else {
                branch
            },
            sha,
            subject,
        })
    }

    /// For `show`, the commit being displayed — diffuse renders this itself
    /// because `--format=` suppresses git's own header.
    pub fn commit_meta(&self) -> Option<CommitMeta> {
        if self.inv.subcommand != Subcommand::Show {
            return None;
        }
        let (head, _) = self.split_pathspec();
        let rev = head
            .iter()
            .find(|a| !a.starts_with('-'))
            .cloned()
            .unwrap_or_else(|| "HEAD".into());
        let raw = self.plumbing(&[
            "show",
            "-s",
            "--format=%H%x00%an%x00%aI%x00%s%x00%b%x00%P",
            &rev,
        ])?;
        let mut parts = raw.split('\0');
        let sha = parts.next().unwrap_or_default().to_string();
        let author = parts.next().unwrap_or_default().to_string();
        let date = parts.next().unwrap_or_default().to_string();
        let subject = parts.next().unwrap_or_default().to_string();
        let body = parts.next().unwrap_or_default().trim_end().to_string();
        let merge = parts
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .count()
            > 1;
        Some(CommitMeta { sha, author, date, subject, body, merge })
    }

    /// Whether the right-hand side of this comparison is the working tree.
    /// Decides whether untracked files belong in the view.
    pub fn worktree_is_right_side(&self) -> bool {
        if self.inv.subcommand != Subcommand::Diff {
            return false;
        }
        let (head, _) = self.split_pathspec();
        if head
            .iter()
            .any(|a| a == "--cached" || a == "--staged")
        {
            return false;
        }
        let mut revs = 0;
        for arg in head.iter().filter(|a| !a.starts_with('-')) {
            // `A..B` and `A...B` compare two commits; the worktree is not involved.
            if arg.contains("..") {
                return false;
            }
            if self
                .plumbing(&["rev-parse", "--verify", "--quiet", &format!("{arg}^{{commit}}")])
                .is_some()
            {
                revs += 1;
            }
        }
        revs < 2
    }
}

/// `add\tdel\tpath\0`, or for renames `add\tdel\t\0old\0new\0`.
/// Binary files report `-` for both counts.
fn parse_numstat_z(text: &str) -> Vec<FileEntry> {
    let mut files = Vec::new();
    let mut fields = text.split('\0');
    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue;
        }
        let mut cols = record.splitn(3, '\t');
        let (Some(add), Some(del), Some(rest)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let binary = add == "-" || del == "-";
        // An empty third column means the two paths of a rename follow.
        let (path, old_path) = if rest.is_empty() {
            let old = fields.next().unwrap_or_default().to_string();
            let new = fields.next().unwrap_or_default().to_string();
            (new, Some(old))
        } else {
            (rest.to_string(), None)
        };
        files.push(FileEntry {
            path,
            old_path,
            status: Status::Modified,
            additions: add.parse().unwrap_or(0),
            deletions: del.parse().unwrap_or(0),
            binary,
            untracked: false,
        });
    }
    files
}

impl Runner {
    /// Untracked files appear in no git diff, so their patch is synthesized:
    /// every line is an addition against an empty left side.
    pub fn untracked_diff(&self, path: &str) -> Result<crate::model::FileDiff, GitError> {
        use crate::model::{FileDiff, Hunk, Line, LineKind};
        let full = self.repo.root.join(path);
        let bytes = std::fs::read(&full)?;
        if bytes.contains(&0) {
            return Ok(FileDiff {
                path: path.to_string(),
                binary: true,
                ..Default::default()
            });
        }
        let text = String::from_utf8_lossy(&bytes);
        let ends_with_newline = text.ends_with('\n');
        let lines: Vec<&str> = if text.is_empty() {
            Vec::new()
        } else {
            text.lines().collect()
        };
        let count = lines.len() as u32;
        let mut out = Vec::with_capacity(lines.len());
        for (i, l) in lines.iter().enumerate() {
            out.push(Line {
                kind: LineKind::Add,
                old_no: None,
                new_no: Some(i as u32 + 1),
                content: (*l).to_string(),
                no_newline: !ends_with_newline && i + 1 == lines.len(),
                words: None,
            });
        }
        let hunks = if count == 0 {
            Vec::new()
        } else {
            vec![Hunk {
                header: format!("@@ -0,0 +1,{count} @@"),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: count,
                section: None,
                lines: out,
            }]
        };
        Ok(FileDiff {
            path: path.to_string(),
            hunks,
            ..Default::default()
        })
    }
}


/// Classify an untracked file and count its lines without loading it.
///
/// git decides "binary" from the first few kilobytes, and so do we: an
/// untracked directory of model weights can easily be hundreds of megabytes,
/// and reading all of it to look for a NUL byte would make listing the files
/// cost more than the diff itself.
fn measure_untracked(path: &Path) -> Option<(u32, bool)> {
    use std::io::{BufRead, BufReader, Read};

    const SNIFF: usize = 8192;
    let file = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut head = vec![0u8; SNIFF];
    let mut filled = 0;
    while filled < SNIFF {
        match reader.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return None,
        }
    }
    if head[..filled].contains(&0) {
        return Some((0, true));
    }
    if filled == 0 {
        return Some((0, false));
    }
    // Count the remaining newlines by streaming, so a large text file costs
    // buffer space rather than its own size in memory.
    let mut lines = head[..filled].iter().filter(|b| **b == b'\n').count() as u32;
    let ended_with_newline = head[..filled].last() == Some(&b'\n');
    let mut tail_ends_with_newline = ended_with_newline;
    let mut saw_more = false;
    while let Ok(chunk) = reader.fill_buf() {
        if chunk.is_empty() {
            break;
        }
        saw_more = true;
        lines += chunk.iter().filter(|b| **b == b'\n').count() as u32;
        tail_ends_with_newline = chunk.last() == Some(&b'\n');
        let n = chunk.len();
        reader.consume(n);
    }
    // A final line without a trailing newline still counts as a line.
    let unterminated = if saw_more { !tail_ends_with_newline } else { !ended_with_newline };
    if unterminated {
        lines += 1;
    }
    Some((lines, false))
}
