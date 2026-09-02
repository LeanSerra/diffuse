# diffuse

<p align="center">
  <img src="web/public/logo.svg" alt="diffuse logo" width="128">
</p>

<p align="center">
  A browser-based pager for git's diff output.
</p>

<p align="center">
  <a href="https://github.com/LeanSerra/diffuse/stargazers">
    <img alt="Stars" src="https://img.shields.io/github/stars/LeanSerra/diffuse?style=flat-square&label=%E2%AD%90%20Stars">
  </a>
  <a href="https://github.com/LeanSerra/diffuse/releases/latest">
    <img alt="Latest release" src="https://img.shields.io/github/v/release/LeanSerra/diffuse?style=flat-square&label=%F0%9F%9A%80%20Release">
  </a>
  <a href="https://github.com/LeanSerra/diffuse/actions/workflows/ci.yml">
    <img alt="CI" src="https://img.shields.io/github/actions/workflow/status/LeanSerra/diffuse/ci.yml?branch=main&style=flat-square&label=%E2%9C%93%20CI">
  </a>
  <a href="LICENSE">
    <img alt="License" src="https://img.shields.io/badge/%F0%9F%93%9C%20License-MIT-blue?style=flat-square">
  </a>
</p>

diffuse is not a git client. Its command line **is** git's: take any
diff-producing `git` command, replace `git` with `diffuse`, and read the result
in a browser instead of a terminal. It opens a tab, renders the patch, and exits
when you close it. Quit it from the terminal instead and the tab says so rather
than going stale.

```sh
diffuse                      # git diff
diffuse main                 # git diff main
diffuse diff main...HEAD     # git diff main...HEAD
diffuse diff --cached -w     # git diff --cached -w
diffuse show <commit>        # git show <commit>
```

## Features

- **Word-level highlighting** — the characters that changed within a line, not just the line.
- **Untracked files included.** `git diff` hides files you have not staged yet, which makes a new file invisible. diffuse synthesizes a patch for them whenever the right-hand side of the comparison is your working tree.
- **Syntax highlighting** over 213 grammars, computed from the whole file so a hunk opening inside a block comment is still a comment.
- **The browser's own find works across the whole diff.** Ctrl+F reaches files you have not scrolled to and files you have collapsed, because every file is in the page rather than loaded as you arrive at it.
- **A commit graph for the range you asked for.** `diffuse diff master` renders the aggregate; press `g` to see the commits that produce it, with merge lanes, and click one to read it on its own. Once you are in a commit, `[` and `]` step to the newer or older one, and each commit you open is a history entry, so the browser's back button walks back through them.
- **A `show` naming several commits is a pager, not a pile.** `diffuse show a b c` and `diffuse show a..b` open on the first commit and step through the rest, rather than stacking each commit's version of a file as its own unlabelled card.
- **Rename-aware.** A rename is shown as a rename, not a delete plus an add.
- **A file list that follows your scroll**, and a file header that stays pinned so a long diff never leaves you guessing.
- **Read-only, by design.** No endpoint writes, so a bug can never cost you work.
- Light and dark, following your system.

`j` / `k` move between files, `b` hides the file list, `r` refreshes.

## Install

Prebuilt binaries for Linux, macOS and Windows are attached to each
[release](https://github.com/LeanSerra/diffuse/releases). To build from source
instead, note the UI is compiled into the binary and has to be built once first:

```sh
cd web && pnpm install && pnpm build && cd ..
cargo install --path .
```

Running diffuse needs only `git` on your `PATH` — Node is a build-time
requirement, not a runtime one.

## Flags

Flags that change what the diff *means* are yours: `-U10`, `-w`, `-M50%`,
`--ignore-blank-lines`, pathspecs, revisions. Flags that produce no patch —
`--stat`, `--name-only`, `--raw`, `--no-patch` — are dropped with a note on
stderr; diffuse always renders the full patch, and the summary those flags
produce is in the sidebar already.

diffuse adds only `--no-color`, `--no-ext-diff` and `core.quotepath=false` to
your command. Each protects the parser rather than changing the diff. Notably
`-M` is *not* forced, because rename detection is already on by default and
forcing it would override anyone who set `diff.renames = false`.

| Flag | Effect |
| --- | --- |
| `--no-untracked` | Do not synthesize diffs for untracked files |
| `--no-open` | Print the URL instead of launching a browser |
| `--inline-all` | Load the whole diff however large, so find reaches every file |

## Security

diffuse serves your source over a loopback port, which any page in your browser
could otherwise read. Four things prevent that: the listener binds to
`127.0.0.1` only, the port is ephemeral, every launch mints a single-use token
carried in the URL, and requests whose `Host` or `Origin` is not loopback are
rejected — which is what stops a DNS-rebinding page from reaching it.

## Development

```sh
cd web && pnpm install && pnpm build   # build the UI
cargo run                              # run against the current repo
cargo test                             # parser snapshots + word-diff tests
```

For UI work, run the API and Vite separately to keep hot reload:

```sh
DIFFUSE_DEV=1 cargo run    # API on :5177, token "dev"
cd web && pnpm dev         # http://localhost:5173/?t=dev
```

`./tests/gen_fixtures.sh` regenerates the parser's corpus of real git output —
renames, mode changes, binaries, missing trailing newlines, non-ASCII paths,
merge commits. A changed snapshot is a changed behaviour and should be read, not
blindly accepted.

## Known limits

- Large diffs render without virtualization, so a few thousand files will be slow. Past 50,000 changed lines the whole diff is no longer loaded up front — find-in-page then reaches only the files you have visited, and the bar says so. `--inline-all` overrides it.
- Combined diffs from merge commits render without old-side line numbers: each `-` belongs to a different parent, so a single number would be wrong.
- Side-by-side view is not implemented.

## License

[MIT](LICENSE)
