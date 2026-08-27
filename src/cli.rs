//! The CLI contract: `diffuse <args>` mirrors `git <args>`.
//!
//! The governing principle is paste-compatibility — any git command must work
//! when you replace `git` with `diffuse`. Flags that cannot apply are dropped
//! and reported on stderr; flags git might add in future are passed through so
//! that only this small ignore-list needs maintenance.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subcommand {
    Diff,
    Show,
}

impl Subcommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Subcommand::Diff => "diff",
            Subcommand::Show => "show",
        }
    }
}

impl fmt::Display for Subcommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub enum Parsed {
    Run(Invocation),
    Help,
    Version,
}

#[derive(Debug, Clone)]
pub struct Invocation {
    pub subcommand: Subcommand,
    /// Arguments forwarded to git verbatim, ignore-list already removed.
    pub args: Vec<String>,
    /// Flags we dropped, reported to the user on stderr.
    pub ignored: Vec<String>,
    /// Whether to synthesize diffs for untracked files (`--no-untracked` opts out).
    pub want_untracked: bool,
    /// Whether to launch a browser. Off for scripts and for driving diffuse
    /// from another tool, which then reads the URL from stderr.
    pub open_browser: bool,
    /// Send the whole diff up front however large it is, so find-in-page can
    /// reach every file. Past the built-in ceiling diffuse otherwise falls
    /// back to loading files as you reach them.
    pub inline_all: bool,
}

impl Invocation {
    /// The command line as the user would have typed it against git, for display.
    pub fn display_command(&self) -> String {
        let mut s = format!("git {}", self.subcommand);
        for a in &self.args {
            s.push(' ');
            s.push_str(a);
        }
        s
    }
}

/// Flags that produce no patch, or a patch our renderer cannot consume.
/// `true` means the flag takes a value in a following argument when not
/// attached with `=`, which must be dropped alongside it.
const IGNORED: &[(&str, bool)] = &[
    // Suppress or replace the patch entirely
    ("-s", false),
    ("--no-patch", false),
    ("--raw", false),
    ("--name-only", false),
    ("--name-status", false),
    ("--check", false),
    ("--quiet", false),
    ("--exit-code", false),
    // Summary formats rendered by our own sidebar instead
    ("--stat", false),
    ("--compact-summary", false),
    ("--numstat", false),
    ("--shortstat", false),
    ("--summary", false),
    ("--dirstat", false),
    ("--dirstat-by-file", false),
    ("--cumulative", false),
    ("--patch-with-raw", false),
    ("--patch-with-stat", false),
    // Inline word diffing — diffuse computes its own, in the renderer
    ("--word-diff", false),
    ("--word-diff-regex", true),
    ("--color-words", false),
    // Colour and machine-readable framing would corrupt the parse
    ("--color", false),
    ("-z", false),
    ("--output", true),
    ("--output-indicator-new", true),
    ("--output-indicator-old", true),
    ("--output-indicator-context", true),
    // Would re-enable an external diff driver, which emits arbitrary output
    ("--ext-diff", false),
];

/// Flags that are redundant because diffuse already guarantees the behaviour.
/// Dropped silently — warning about them would be noise, since the result is
/// exactly what the user asked for.
const SILENT: &[&str] = &["--no-color", "-p", "-u", "--patch"];

fn lookup(flag: &str) -> Option<(&'static str, bool)> {
    // `--stat=80` and `--stat` are the same flag for our purposes.
    let name = flag.split('=').next().unwrap_or(flag);
    IGNORED.iter().copied().find(|(f, _)| *f == name)
}

pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Parsed {
    let argv: Vec<String> = argv.into_iter().collect();

    if argv.iter().any(|a| a == "-h" || a == "--help") {
        return Parsed::Help;
    }
    if argv.iter().any(|a| a == "-V" || a == "--version") {
        return Parsed::Version;
    }

    // `diffuse diff ...` / `diffuse show ...`, or the bare form `diffuse <rev>`
    // which is shorthand for `diffuse diff <rev>`.
    let (subcommand, rest): (Subcommand, &[String]) = match argv.first().map(String::as_str) {
        Some("diff") => (Subcommand::Diff, &argv[1..]),
        Some("show") => (Subcommand::Show, &argv[1..]),
        _ => (Subcommand::Diff, &argv[..]),
    };

    let mut args = Vec::new();
    let mut ignored = Vec::new();
    let mut want_untracked = true;
    let mut open_browser = true;
    let mut inline_all = false;
    let mut past_separator = false;
    let mut skip_next = false;

    for arg in rest {
        if skip_next {
            skip_next = false;
            continue;
        }

        // Everything after `--` is a pathspec and must never be interpreted.
        if past_separator {
            args.push(arg.clone());
            continue;
        }
        if arg == "--" {
            past_separator = true;
            args.push(arg.clone());
            continue;
        }

        if arg == "--no-untracked" {
            want_untracked = false;
            continue;
        }
        if arg == "--no-open" {
            open_browser = false;
            continue;
        }
        if arg == "--inline-all" {
            inline_all = true;
            continue;
        }

        if arg.starts_with('-') {
            if SILENT.contains(&arg.as_str()) {
                continue;
            }
            if let Some((_, takes_value)) = lookup(arg) {
                ignored.push(arg.clone());
                // `--output file` must lose its value too, or the value would
                // be reinterpreted as a revision.
                skip_next = takes_value && !arg.contains('=');
                continue;
            }
        }

        args.push(arg.clone());
    }

    Parsed::Run(Invocation {
        subcommand,
        args,
        ignored,
        want_untracked,
        open_browser,
        inline_all,
    })
}

pub const HELP: &str = "\
diffuse — a browser-based pager for git's diff output

USAGE:
    diffuse [<git-diff-args>...]
    diffuse diff [<args>...]
    diffuse show [<args>...]

Any diff-producing git command works by replacing `git` with `diffuse`:

    diffuse                      same as  git diff
    diffuse main                 same as  git diff main
    diffuse diff main...HEAD     same as  git diff main...HEAD
    diffuse diff --cached -w     same as  git diff --cached -w
    diffuse show <commit>        same as  git show <commit>

OPTIONS:
    --no-untracked    Do not synthesize diffs for untracked files
    --no-open         Print the URL instead of opening a browser
    --inline-all      Send the whole diff up front however large, so the
                      browser's own find reaches every file
    -h, --help        Show this help
    -V, --version     Show version

Flags that produce no patch (--stat, --name-only, --raw, ...) are ignored with
a warning; diffuse always renders the full patch.
";
