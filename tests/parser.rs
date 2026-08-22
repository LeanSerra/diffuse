//! Snapshot tests over a corpus of real git output.
//!
//! Regenerate the corpus with `./tests/gen_fixtures.sh` after changing what
//! diffuse asks git for. A snapshot change is a real behaviour change and
//! should be reviewed, not blindly accepted.

use diffuse::parse::{parse_patch, unquote_path};

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/patches")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    String::from_utf8_lossy(&bytes).into_owned()
}

macro_rules! snapshot_fixture {
    ($test:ident, $file:literal) => {
        #[test]
        fn $test() {
            let files = parse_patch(&fixture($file));
            insta::assert_json_snapshot!(files);
        }
    };
}

snapshot_fixture!(modify, "01-modify.patch");
snapshot_fixture!(hunks_with_section, "02-hunks-section.patch");
snapshot_fixture!(added_and_empty, "03-added.patch");
snapshot_fixture!(deleted, "04-deleted.patch");
snapshot_fixture!(renames, "05-renames.patch");
snapshot_fixture!(mode_only, "06-mode-only.patch");
snapshot_fixture!(binary, "07-binary.patch");
snapshot_fixture!(binary_payload, "08-binary-payload.patch");
snapshot_fixture!(nonewline, "09-nonewline.patch");
snapshot_fixture!(nonewline_gained, "10-nonewline-gained.patch");
snapshot_fixture!(odd_paths, "11-odd-paths.patch");
snapshot_fixture!(crlf, "12-crlf.patch");
snapshot_fixture!(combined_merge, "13-combined-merge.patch");
snapshot_fixture!(show_with_header, "14-show-with-header.patch");

#[test]
fn unquotes_octal_escaped_utf8() {
    // Two octal escapes forming one multi-byte character.
    assert_eq!(unquote_path(r#""caf\303\251.txt""#), "café.txt");
    assert_eq!(unquote_path(r#""with\"quote.txt""#), "with\"quote.txt");
    assert_eq!(unquote_path(r#""tab\there.txt""#), "tab\there.txt");
    assert_eq!(unquote_path("plain.txt"), "plain.txt");
}

/// Line numbers must stay aligned with the hunk header, which is what makes a
/// diff navigable at all.
#[test]
fn line_numbers_track_hunk_headers() {
    for name in ["01-modify.patch", "02-hunks-section.patch", "12-crlf.patch"] {
        for file in parse_patch(&fixture(name)) {
            for hunk in &file.hunks {
                let olds = hunk.lines.iter().filter_map(|l| l.old_no).collect::<Vec<_>>();
                let news = hunk.lines.iter().filter_map(|l| l.new_no).collect::<Vec<_>>();
                if let Some(&first) = olds.first() {
                    assert_eq!(first, hunk.old_start, "{name}: old start");
                }
                if let Some(&first) = news.first() {
                    assert_eq!(first, hunk.new_start, "{name}: new start");
                }
                assert_eq!(olds.len() as u32, hunk.old_lines, "{name}: old count");
                assert_eq!(news.len() as u32, hunk.new_lines, "{name}: new count");
                for w in olds.windows(2) {
                    assert_eq!(w[1], w[0] + 1, "{name}: old numbering gap");
                }
                for w in news.windows(2) {
                    assert_eq!(w[1], w[0] + 1, "{name}: new numbering gap");
                }
            }
        }
    }
}

/// A combined diff cannot carry a meaningful old-side line number: each `-`
/// marker belongs to a different parent. Reporting none beats reporting a
/// wrong one.
#[test]
fn combined_diff_omits_old_numbers_and_numbers_the_result() {
    let files = parse_patch(&fixture("13-combined-merge.patch"));
    let file = files.iter().find(|f| f.combined).expect("combined file");
    let mut expected = None;
    for hunk in &file.hunks {
        for line in &hunk.lines {
            assert!(line.old_no.is_none(), "combined diff must not guess old numbers");
        }
        let news: Vec<u32> = hunk.lines.iter().filter_map(|l| l.new_no).collect();
        let start = *expected.get_or_insert(hunk.new_start);
        assert_eq!(news.first().copied(), Some(start));
        for w in news.windows(2) {
            assert_eq!(w[1], w[0] + 1, "result side must be contiguous");
        }
    }
}
