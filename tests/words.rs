//! Word-level highlighting, verified by rendering the marked ranges beneath
//! the line so a wrong pairing is visible rather than merely stable.

use diffuse::model::LineKind;
use diffuse::parse::parse_patch;
use diffuse::words::annotate;

/// Render a line with `~` under every highlighted range.
fn render(patch: &str) -> String {
    let mut files = parse_patch(patch);
    let mut out = String::new();
    for f in files.iter_mut() {
        annotate(&mut f.hunks);
        for h in &f.hunks {
            for l in &h.lines {
                let sigil = match l.kind {
                    LineKind::Add => '+',
                    LineKind::Del => '-',
                    LineKind::Context => ' ',
                };
                out.push(sigil);
                out.push_str(&l.content);
                out.push('\n');
                if let Some(words) = &l.words {
                    let units: Vec<u16> = l.content.encode_utf16().collect();
                    let mut bar: Vec<char> = vec![' '; units.len() + 1];
                    for r in words {
                        for c in bar.iter_mut().take(r.end.min(units.len())).skip(r.start) {
                            *c = '~';
                        }
                    }
                    out.push_str(bar.iter().collect::<String>().trim_end());
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn patch(body: &str) -> String {
    format!(
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,{n} +1,{n} @@\n{body}",
        n = body.lines().count()
    )
}

#[test]
fn highlights_only_the_changed_word() {
    insta::assert_snapshot!(render(&patch(
        "-let total = price * quantity;\n+let total = price * qty;\n"
    )));
}

#[test]
fn handles_insertion_and_deletion_within_a_line() {
    insta::assert_snapshot!(render(&patch("-fn run(a, c)\n+fn run(a, b, c)\n")));
}

#[test]
fn full_rewrite_is_left_unhighlighted() {
    // Marking the entire line adds noise without adding information.
    let out = render(&patch("-alpha bravo charlie\n+zulu yankee xray\n"));
    assert!(!out.contains('~'), "expected no highlights, got:\n{out}");
}

#[test]
fn unequal_runs_are_not_paired() {
    // 1 deletion replaced by 3 additions: any pairing would be a guess.
    let out = render(&patch("-one\n+one\n+two\n+three\n"));
    assert!(!out.contains('~'), "expected no highlights, got:\n{out}");
}

#[test]
fn offsets_are_utf16_so_the_browser_can_slice_directly() {
    // The emoji is two UTF-16 units but one Rust char; a char-index offset
    // would put the highlight one unit to the left.
    let rendered = render(&patch("-x 🎉 alpha\n+x 🎉 bravo\n"));
    insta::assert_snapshot!(rendered);
}
