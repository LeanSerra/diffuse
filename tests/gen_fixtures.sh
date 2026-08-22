#!/usr/bin/env bash
# Regenerate the parser's fixture corpus from real git output.
# Run from the repo root:  ./tests/gen_fixtures.sh
set -euo pipefail

OUT="$(cd "$(dirname "$0")" && pwd)/patches"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
rm -f "$OUT"/*.patch

cd "$TMP"
git init -q .
git config user.email fixture@diffuse
git config user.name fixture
git config commit.gpgsign false

# `-c core.quotepath=false --no-color --no-ext-diff` mirrors what diffuse injects.
g() { git -c core.quotepath=false "$@" --no-color --no-ext-diff; }

# --- baseline commit -------------------------------------------------------
printf 'alpha\nbravo\ncharlie\ndelta\n' > simple.txt
printf 'int main(void)\n{\n\tint x = 1;\n\treturn x;\n}\n\nvoid helper(void)\n{\n\tint y = 2;\n\t(void)y;\n}\n' > code.c
printf 'to be deleted\n' > gone.txt
printf 'original content here\nsecond line\nthird line\n' > renameme.txt
printf 'chmod target\n' > exec.sh
printf 'no trailing newline' > nonewline.txt
printf 'x\n' > "café.txt"
printf 'y\n' > "with space.txt"
printf '\x00\x01\x02\x03binary\n' > blob.bin
git add -A && git commit -qm baseline

# --- 1. simple modification ------------------------------------------------
printf 'alpha\nBRAVO\ncharlie\ndelta\n' > simple.txt
g diff -- simple.txt > "$OUT/01-modify.patch"
git checkout -q -- simple.txt

# --- 2. multiple hunks with function section headings ----------------------
printf 'int main(void)\n{\n\tint x = 42;\n\treturn x;\n}\n\nvoid helper(void)\n{\n\tint y = 99;\n\t(void)y;\n}\n' > code.c
g diff -U1 -- code.c > "$OUT/02-hunks-section.patch"
git checkout -q -- code.c

# --- 3. new file, and an empty new file ------------------------------------
printf 'brand new\nsecond\n' > added.txt
: > empty.txt
git add added.txt empty.txt
g diff --cached -- added.txt empty.txt > "$OUT/03-added.patch"

# --- 4. deleted file -------------------------------------------------------
git rm -q gone.txt
g diff --cached -- gone.txt > "$OUT/04-deleted.patch"

# --- 5. rename with content change, and a pure rename ----------------------
git mv renameme.txt renamed.txt
printf 'original content here\nSECOND LINE\nthird line\n' > renamed.txt
git add renamed.txt
cp "with space.txt" moved-verbatim.txt && git rm -q "with space.txt" && git add moved-verbatim.txt
g diff --cached -M -- renameme.txt renamed.txt "with space.txt" moved-verbatim.txt > "$OUT/05-renames.patch"

# --- 6. mode change only ---------------------------------------------------
chmod +x exec.sh
g diff -- exec.sh > "$OUT/06-mode-only.patch"

# --- 7. binary --------------------------------------------------------------
printf '\x00\x01\x02\x03CHANGED\n' > blob.bin
g diff -- blob.bin > "$OUT/07-binary.patch"
g diff --binary -- blob.bin > "$OUT/08-binary-payload.patch"
git checkout -q -- blob.bin

# --- 8. no trailing newline, both directions -------------------------------
printf 'no trailing newline CHANGED' > nonewline.txt
g diff -- nonewline.txt > "$OUT/09-nonewline.patch"
printf 'now it has one\n' > nonewline.txt
g diff -- nonewline.txt > "$OUT/10-nonewline-gained.patch"
git checkout -q -- nonewline.txt

# --- 9. non-ASCII path, and a path containing a quote ----------------------
printf 'x\nchanged\n' > "café.txt"
printf 'q\n' > 'weird"name.txt'
git add 'weird"name.txt'
g diff HEAD -- "café.txt" 'weird"name.txt' > "$OUT/11-odd-paths.patch"
git checkout -q -- "café.txt"; git rm -qf --cached 'weird"name.txt'; rm -f 'weird"name.txt'

# --- 10. CRLF content ------------------------------------------------------
printf 'one\r\ntwo\r\nthree\r\n' > crlf.txt
git add crlf.txt && git commit -qm crlf
printf 'one\r\nTWO\r\nthree\r\n' > crlf.txt
g diff -- crlf.txt > "$OUT/12-crlf.patch"
git checkout -q -- crlf.txt

# --- 11. merge commit, combined diff ---------------------------------------
git checkout -q -b side HEAD
printf 'alpha\nSIDE\ncharlie\ndelta\n' > simple.txt
git commit -qam side-change
git checkout -q -
printf 'alpha\nMAIN\ncharlie\ndelta\n' > simple.txt
git commit -qam main-change
git merge side -q 2>/dev/null || true          # conflicts on purpose
printf 'alpha\nRESOLVED\ncharlie\ndelta\n' > simple.txt
git add simple.txt && git commit -qm "merge side"
git show --cc --format= HEAD > "$OUT/13-combined-merge.patch"

# --- 12. `git show` preamble that must be skipped --------------------------
git show HEAD~1 --no-color --no-ext-diff > "$OUT/14-show-with-header.patch"

echo "wrote $(ls -1 "$OUT" | wc -l) fixtures to $OUT"
