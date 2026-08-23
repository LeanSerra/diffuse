//! Lane assignment, checked by drawing the graph as text so a wrong lane is
//! visible rather than merely stable.

use diffuse::graph::lay_out;

fn draw(spec: &[(&str, &[&str])]) -> String {
    let commits: Vec<(&str, Vec<&str>)> =
        spec.iter().map(|(s, p)| (*s, p.to_vec())).collect();
    let nodes = lay_out(&commits);
    let width = nodes.iter().map(|n| n.width).max().unwrap_or(1);
    let mut out = String::new();
    for (node, (sha, parents)) in nodes.iter().zip(spec) {
        let mut row = vec![' '; width * 2];
        for lane in &node.through {
            row[lane * 2] = '|';
        }
        for (from, to) in &node.edges {
            if from != to {
                let (a, b) = (from.min(to), from.max(to));
                for cell in row.iter_mut().take(b * 2).skip(a * 2 + 1) {
                    if *cell == ' ' {
                        *cell = '_';
                    }
                }
            }
        }
        row[node.lane * 2] = '*';
        out.push_str(row.iter().collect::<String>().trim_end());
        out.push_str(&format!("  {sha} -> {}\n", parents.join(",")));
    }
    out
}

#[test]
fn linear_history_is_one_lane() {
    insta::assert_snapshot!(draw(&[
        ("d", &["c"]),
        ("c", &["b"]),
        ("b", &["a"]),
        ("a", &[]),
    ]));
}

#[test]
fn a_merge_opens_a_second_lane_and_closes_it() {
    // m merges the side branch s into the main line; both rejoin at base.
    insta::assert_snapshot!(draw(&[
        ("m", &["main1", "s1"]),
        ("main1", &["base"]),
        ("s1", &["base"]),
        ("base", &[]),
    ]));
}

#[test]
fn two_side_branches_get_distinct_lanes() {
    insta::assert_snapshot!(draw(&[
        ("m2", &["m1", "b1"]),
        ("m1", &["m0", "a1"]),
        ("b1", &["base"]),
        ("a1", &["base"]),
        ("m0", &["base"]),
        ("base", &[]),
    ]));
}

/// The root has no parents, so every lane must be closed by the end.
#[test]
fn lanes_all_close_by_the_root() {
    let spec: Vec<(&str, Vec<&str>)> = vec![
        ("m", vec!["a", "b"]),
        ("a", vec!["root"]),
        ("b", vec!["root"]),
        ("root", vec![]),
    ];
    let nodes = lay_out(&spec);
    let last = nodes.last().unwrap();
    assert!(last.through.is_empty(), "no lane may outlive the root: {last:?}");
    assert!(last.edges.is_empty(), "the root has no parents to point at");
}

/// Every commit must be drawn in the lane its child pointed at, or the lines
/// join the wrong dots.
#[test]
fn edges_land_on_the_lane_the_commit_uses() {
    let spec: Vec<(&str, Vec<&str>)> = vec![
        ("m", vec!["a", "b"]),
        ("a", vec!["c"]),
        ("b", vec!["c"]),
        ("c", vec![]),
    ];
    let nodes = lay_out(&spec);
    let lane_of: std::collections::HashMap<&str, usize> = spec
        .iter()
        .zip(&nodes)
        .map(|((sha, _), n)| (*sha, n.lane))
        .collect();
    for ((_, parents), node) in spec.iter().zip(&nodes) {
        for (parent, (_, to)) in parents.iter().zip(&node.edges) {
            assert_eq!(
                lane_of[parent], *to,
                "edge for {parent} points at lane {to} but it is drawn in {}",
                lane_of[parent],
            );
        }
    }
}
