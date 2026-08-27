//! The CLI is the contract: any diff-producing git command has to survive
//! `git` being swapped for `diffuse`. These cover diffuse's own flags, which
//! are the only arguments that must *not* reach git.

use diffuse::cli::{parse, Parsed, Subcommand};

fn run(args: &[&str]) -> diffuse::cli::Invocation {
    match parse(args.iter().map(|s| s.to_string())) {
        Parsed::Run(i) => i,
        other => panic!("expected a run, got {other:?}"),
    }
}

#[test]
fn inline_all_is_diffuses_own_and_never_reaches_git() {
    let i = run(&["diff", "--inline-all", "main"]);
    assert!(i.inline_all);
    assert_eq!(i.args, vec!["main"]);
    assert_eq!(i.subcommand, Subcommand::Diff);
}

#[test]
fn inline_all_defaults_off() {
    assert!(!run(&["diff", "main"]).inline_all);
}

#[test]
fn a_pathspec_named_like_a_flag_is_still_a_pathspec() {
    // Everything after `--` belongs to git untouched, even a word diffuse
    // would otherwise claim for itself.
    let i = run(&["diff", "--", "--inline-all"]);
    assert!(!i.inline_all);
    assert_eq!(i.args, vec!["--", "--inline-all"]);
}
