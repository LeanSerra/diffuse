# diffuse — Plan (v1, post-grill)

**diffuse is a browser-based pager for git's diff output.**

It is not an app that knows about git and builds its own commands. Its CLI
surface *is* git's: any diff-producing `git …` command becomes a `diffuse …`
command by swapping one word. `diffuse` opens a local server, renders the patch
in a browser with a GitHub-style reading experience, and exits when you're done.

This reframing is the single most important decision here. It collapses what
looked like a v2 feature — branch-vs-branch browsing — into an invocation:
`diffuse diff main...HEAD`. There is no branch-compare subsystem to build.

---

## 1. CLI contract

### Paste-compatibility is the contract

Any `git` command must work when you replace `git` with `diffuse`. No errors on
flags that don't apply, no surprises. This is the governing principle; where it
conflicts with purity, it wins.

```
diffuse                      ==  git diff
diffuse main                 ==  git diff main          (bare revision shorthand)
diffuse diff main...HEAD     ==  git diff main...HEAD
diffuse diff --cached -w     ==  git diff --cached -w
diffuse show <oid>           ==  git show <oid>
diffuse diff -U10 -- src/    ==  git diff -U10 -- src/
```

### Subcommand allowlist

v1 accepts **`diff`** and **`show`** only. The allowlist is what enforces the
read-only guarantee (§5) — blind passthrough would make `diffuse add .` mutate
the repo. `log -p` is deferred: it produces a *list of commits*, which is a
different UI, not a different diff.

### Flag handling

| Class | Examples | Behavior |
|---|---|---|
| Injected | `--no-color`, `--no-ext-diff`, and `-c core.quotepath=false` (top-level, before the subcommand) | Always applied |
| Honored | `-U<n>`, `-w`, `-M`, `--ignore-blank-lines`, pathspecs, revisions | Passed through |
| Ignored | `--stat`, `--name-only`, `--raw`, `--no-patch`, `--word-diff`, `--color` | Dropped, warned on stderr |
| Unknown | anything git adds in future | Passed through — if git rejects it, startup validation (§4) surfaces git's own error |

**The injection rule: inject only what protects the parser or the transport;
never inject anything that changes what the diff means.** Each of the three
earns its place, verified empirically:

- `--no-color` — a user with `color.ui = always` gets ANSI escapes *even through
  a pipe* (measured: 9 escape sequences in a 3-line diff). Would poison every line.
- `--no-ext-diff` — with `diff.external` configured, `git diff` emits whatever
  that program prints and no unified diff at all. Without this flag the parser
  receives arbitrary bytes. It does **not** disable `textconv`, and must not:
  textconv still produces a valid unified diff and is how users make binary
  files readable. Never add `--no-textconv`.
- `-c core.quotepath=false` — otherwise non-ASCII paths arrive octal-escaped as
  `diff --git "a/caf\303\251.txt"` instead of `diff --git a/café.txt`. The parser
  must *still* handle C-style quoting, since paths containing `"` or newlines are
  quoted regardless.

Two flags were considered and rejected. `--no-pager` does nothing: git only
invokes a pager when stdout is a TTY, and diffuse always captures a pipe.
`-M` fails the injection rule: rename detection is already on by default
(`diff.renames`, git ≥2.9), and forcing it *overrides* a user who explicitly set
`diff.renames=false` — changing diff content behind their back, which is exactly
what paste-compatibility forbids.

Passing unknown flags through is deliberate: it means only the small ignore-list
needs maintenance as git evolves, and unrecognized-flag behavior stays identical
to git's.

Ignored flags are reported **on stderr in the terminal, before the browser
opens** — that's where the person who typed the command is looking. Nothing is
shown in the UI.

### Untracked files

git excludes untracked files from every diff mode, so a brand-new file would be
invisible. diffuse synthesizes a diff against `/dev/null` so it renders as
all-additions, respecting `.gitignore`.

**Rule: include untracked iff the right-hand side of the diff is the working
tree.** True for bare `diffuse` and `diffuse diff main`; false for `--cached`,
for `diff A B`, and for `show`. Mechanically detectable from argv, no heuristics.
`--no-untracked` opts out.

One flag was added during implementation: `--no-open` prints the URL rather
than launching a browser. It exists because driving diffuse from a script (or a
test) is otherwise impossible — `opener` goes through `xdg-open` on Linux and
ignores `BROWSER`.

---

## 2. Architecture

```
browser (React)  ──HTTP──▶  axum server (127.0.0.1)  ──execFile──▶  git
```

**Backend: Rust.** `cargo install` / Homebrew / release binaries mean a single
static file with zero runtime dependencies. "Requires Node" is exactly the
friction that stops people installing a small git tool.

**Frontend: React + TypeScript + Vite**, plain CSS with custom properties. Bundle
size is irrelevant when the binary serves it locally; the virtualization
ecosystem (needed later, §6) is what matters.

**Why shell out to git rather than use `gitoxide`:** the CLI contract requires
git's full revision syntax (`main...HEAD`, `HEAD@{2}`, `:/fix typo`).
Reimplementing that on top of a library is a multi-month project. git is the
parser.

### Crates
`axum` 0.8, `rust-embed` 8, `opener` 0.8, `getrandom` 0.4, `insta` 1.48 (dev).
No diff-parsing crate — see §3. The session token takes `getrandom` rather than
`rand`: it needs 16 bytes from the OS, not a generator framework, and that drops
the `rand_chacha`/`ppv-lite86`/`zerocopy` subtree with it.

---

## 3. Diff pipeline

**Hand-rolled unified-diff parser** (~300 lines). Every candidate crate is 0.x,
and this parser is the core correctness of the entire product — it's the wrong
dependency to take.

Must survive: `\ No newline at end of file`, mode-only changes, rename/copy with
similarity index, `Binary files … differ` vs `GIT binary patch`, combined diffs
from merge commits (`@@@`, two marker columns), quoted non-ASCII paths, and the
`@@ … @@ <section heading>` trailer.

**Stats come from `git diff --numstat -z`**, not from counting parsed lines.
numstat is authoritative and NUL-delimited, so it survives paths containing
newlines.

### Lazy fetch: stateless re-run

diffuse holds no diff state. The sidebar comes from one `--numstat -z` call;
each file's body is fetched on demand by re-running **the same argv with
`-- <path>` appended**. Flat memory regardless of repo size, and it preserves the
pager mental model: diffuse is a window onto whatever git says right now.

Tradeoff accepted: for working-tree diffs, files opened at different times may
not be mutually consistent if you edit while browsing. That's what refresh is
for, and every commit-to-commit diff is immutable anyway.

---

## 4. Errors and process lifetime

**Startup validation behaves exactly like git.** `diffuse diff nonexistent-branch`
prints git's stderr, exits non-zero, and never opens a browser. Opening a tab to
say "bad revision" is worse than useless when you're standing in a terminal.
Errors that occur *in-session* (refreshing after a branch is deleted) render in
the UI, because by then the browser is where you are.

**Lifetime:** runs until Ctrl-C, plus an SSE heartbeat that exits ~30s after the
last client disconnects. Closing the tab cleans up, without the fragility of
exiting the instant a connection blips.

---

## 5. Security

diffuse is **read-only, permanently**. There are no mutating endpoints. A bug
can never destroy work, and the whole server is a pure function of repo state.
If you want to stage hunks, that's a different tool.

A localhost server serving your source code is readable by any webpage open in
your browser — a malicious tab can `fetch('http://127.0.0.1:PORT/api/files')`
and exfiltrate your working tree. Mitigations, all in from day one because
retrofitting them means touching every endpoint:

1. Bind `127.0.0.1` only
2. Random ephemeral port (`:0`)
3. Per-launch session token, passed in the URL diffuse opens
4. Reject requests whose `Origin`/`Host` don't match

---

## 6. HTTP API

```
GET /api/session          → { repoRoot, name, head: {branch, sha, subject},
                              argv, worktreeRight: bool, ignoredFlags: [] }
GET /api/files            → { files: [{ path, oldPath, status, additions,
                              deletions, binary, untracked }], stats }
GET /api/file?path=       → { hunks: Hunk[], binary, truncated }
GET /api/events           → SSE heartbeat (drives §4 lifetime)
```

```ts
Hunk = { header, oldStart, oldLines, newStart, newLines, lines: Line[] }
Line = { type: 'context'|'add'|'del', oldNo?, newNo?, content,
         noNewline?, words?: Range[] }
```

The client never sees raw diff text.

---

## 7. UI

**Continuous scroll**, GitHub-style: every file stacked in one scrolling column,
sticky header per file. Chosen over master/detail deliberately, accepting that
it will be slow on large diffs at first — virtualization is a later optimization
made against real measurements, not a guess up front.

- File bodies lazy-fetched as they approach the viewport
- Unloaded files reserve space estimated from their `+n −m` counts. A fixed-size
  placeholder makes the document far shorter than it will become, so scrolling
  to a file near the end clamps at the bottom and lands nowhere near it — the
  reason clicking a file used to take several attempts. A jump is additionally
  held on its target for a moment while surrounding files load, and released the
  instant you scroll yourself.
- Files over ~5k lines render collapsed behind a "load anyway" click
- Files are stacked flush, with no gap between cards. A sticky header only
  holds while its own card is on screen, so any space between cards is a window
  where the previous header has been pushed off and the next has not arrived —
  and the diff shows through at the top edge while scrolling.
- Left: file list — status badge, path (directory dimmed, basename bright), `+n −m`
- Unified view with two line-number gutters, monospace, `tab-size` honored
- **Word-level intra-line diff** — char-level Myers over paired +/− lines. The
  single biggest readability win for ~50 lines of code.
- `j`/`k` between files, `r` to refresh
- Collapsing a file only moves the page when its header has scrolled off the
  top — the case where the rows you were reading are about to disappear.
  Collapsing a file whose header is on screen leaves the scroll position alone.
- Top bar: repo name, branch, the resolved command, refresh
- Manual refresh only. Auto-refresh would only ever apply to working-tree diffs,
  and filesystem watching is noisy (`cargo build` fires hundreds of events).

A per-file change map — a minimap of where each file's hunks fall — was built
and then removed. It read as decoration rather than navigation, and the sticky
file header, the `+n −m` counts and the scroll-tracking file list already
answer "where am I" without it.

**No syntax highlighting in v1.** Highlight spans must survive being split across
added/removed lines, which entangles it with the diff renderer. Its absence
reads as "plain", not "broken". **No split view** — GitHub's own default is
unified.

---

## 8. Build and distribution

- One cargo crate + a `web/` Vite app; `rust-embed` bakes `web/dist` into the binary
- `DIFFUSE_DEV=1` proxies to Vite on 5173 instead of serving embedded assets, preserving HMR
- Release: GitHub Actions binaries + Homebrew as the primary channel
- `web/dist` is committed **on tagged releases only** (release script builds,
  commits, tags) so `cargo install` works without a bundle diff on every commit.
  A tool for reading diffs should not have an unreadable diff in its own history.

---

## 9. Testing

A shell script builds throwaway repos exercising every quirk in §3 — rename,
mode change, binary, missing trailing newline, non-ASCII path, merge commit,
empty file — captured as `insta` snapshots. That's the difference between a
viewer people trust and one that silently mangles a rename. No e2e in v1.

---

## 10. Scope

**v1 done means:** `diffuse`, `diffuse <rev>`, `diffuse diff <args>`, and
`diffuse show <oid>` all render correct, browsable, word-diffed patches with
untracked files included where appropriate, and the process cleans itself up.

**Deferred:** `log -p` / commit-list UI, interactive branch picker, split view,
syntax highlighting, virtualization, `--watch`, multi-repo tabs.

**Never:** staging, discarding, committing, or any other write.

---

## 11. Remaining risks

1. Continuous scroll without virtualization will be slow on 2,000-file diffs.
   Accepted knowingly (§7); revisit with measurements, not guesses.
2. Combined diffs from merge commits are the parser's hardest case. v1 must at
   minimum not corrupt them; degraded rendering is acceptable.
3. Token-in-URL lands in browser history. Acceptable for a per-launch ephemeral
   token bound to a port that won't exist next time.
4. The ignore-list of format-mangling flags needs upkeep as git evolves —
   mitigated by passing unknown flags through (§1).
5. Stateless re-run means N git invocations for N files. Fast on warm repos,
   unmeasured on cold ones over network filesystems.
