# diffuse

A browser-based pager for git's diff output.

diffuse is not a git client. Its command line **is** git's: take any
diff-producing `git` command, replace `git` with `diffuse`, and read the result
in a browser instead of a terminal.

```sh
diffuse                      # git diff
diffuse main                 # git diff main
diffuse diff main...HEAD     # git diff main...HEAD
diffuse diff --cached -w     # git diff --cached -w
diffuse show <commit>        # git show <commit>
```

It opens a tab, renders the patch, and exits when you close it.

## What you get

- **Word-level highlighting** — the characters that changed within a line, not
  just the line.
- **Untracked files included.** `git diff` hides files you have not staged yet,
  which makes a brand-new file invisible. diffuse synthesizes a patch for them
  whenever the right-hand side of the comparison is your working tree.
- **Rename-aware.** A rename is shown as a rename, not as a delete plus an add.
- **A sticky file header** that pins the file you are reading to the top of the
  window, so a long diff never leaves you guessing.
- Light and dark, following your system.

The file list tracks your scroll position, so it always shows where you are.
`j` / `k` move between files, `b` hides the list, `r` refreshes.

## Install

```sh
cargo install --path .
```

Requires `git` on your `PATH`. No Node runtime is needed at install time.

## Read-only, on purpose

diffuse has no endpoint that writes. It cannot stage, discard, or commit
anything — the whole server is a pure function of your repository's current
state, so a bug in it can never cost you work.

It also serves your source code over a loopback port, which any page in your
browser could otherwise read. Four things prevent that: the listener binds to
`127.0.0.1` only, the port is ephemeral, every launch mints a single-use token
carried in the URL, and requests whose `Host` or `Origin` is not loopback are
rejected — which is what stops a DNS-rebinding page from reaching it.

## Flags

Flags that change what the diff *means* are yours: `-U10`, `-w`, `-M50%`,
`--ignore-blank-lines`, pathspecs, revisions. Flags that produce no patch —
`--stat`, `--name-only`, `--raw`, `--no-patch` — are dropped, with a note on
stderr saying which. diffuse always renders the full patch; the summary those
flags produce is in the sidebar already.

diffuse adds only `--no-color`, `--no-ext-diff` and `core.quotepath=false` to
your command. Each protects the parser rather than changing the diff: colour
escapes appear even through a pipe when `color.ui = always`, an external diff
driver replaces the patch with arbitrary output, and non-ASCII paths otherwise
arrive octal-escaped. Notably `-M` is *not* forced, because rename detection is
already on by default and forcing it would override anyone who set
`diff.renames = false`.

`--no-untracked` turns off untracked-file synthesis. `--no-open` prints the
URL on stdout instead of launching a browser, so diffuse can be driven from a
script or another tool.

## Development

```sh
cd web && pnpm install && pnpm build   # build the UI
cargo run                              # run against the current repo
cargo test                             # parser snapshots + word-diff tests
```

For UI work, run the API and Vite separately so you keep hot reload:

```sh
DIFFUSE_DEV=1 cargo run    # API on :5177, token "dev"
cd web && pnpm dev         # http://localhost:5173/?t=dev
```

`./tests/gen_fixtures.sh` regenerates the parser's corpus of real git output —
renames, mode changes, binaries, missing trailing newlines, non-ASCII paths,
merge commits. A changed snapshot is a changed behaviour and should be read,
not blindly accepted.

## Known limits

- Large diffs are rendered without virtualization, so a few thousand files will
  be slow. Files over 5,000 changed lines wait behind a click.
- Combined diffs from merge commits render without old-side line numbers: each
  `-` belongs to a different parent, so a single number would be wrong.
- Syntax highlighting and side-by-side view are not implemented.
