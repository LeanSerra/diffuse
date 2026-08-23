//! Lane assignment for the commit graph.
//!
//! Commits arrive newest first, as git logs them. Each row occupies a column
//! ("lane"); a lane stays alive from the row that opens it down to the row of
//! the commit it is waiting for. Merges open extra lanes, and a lane closes
//! when its awaited commit is drawn.
//!
//! This is pure: it takes shas and parent shas and returns geometry, so the
//! topology can be tested without a repository.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Node {
    /// Column this commit is drawn in.
    pub lane: usize,
    /// One edge per parent, as (this lane -> the lane the parent will occupy).
    pub edges: Vec<(usize, usize)>,
    /// Lanes that pass straight through this row without touching it.
    pub through: Vec<usize>,
    /// Widest lane index in play on this row, so the renderer can size itself.
    pub width: usize,
}

/// `commits` is (sha, parent shas), newest first.
pub fn lay_out<S: AsRef<str>>(commits: &[(S, Vec<S>)]) -> Vec<Node> {
    // lanes[i] is the sha lane i is currently waiting to draw.
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut out = Vec::with_capacity(commits.len());

    for (sha, parents) in commits {
        let sha = sha.as_ref();

        // A commit is drawn in the lane already waiting for it, or in a new one
        // if nothing referenced it — which is how the first row, and any
        // unreferenced tip, gets placed.
        let lane = match lanes.iter().position(|l| l.as_deref() == Some(sha)) {
            Some(i) => i,
            None => {
                let free = lanes.iter().position(Option::is_none);
                match free {
                    Some(i) => i,
                    None => {
                        lanes.push(None);
                        lanes.len() - 1
                    }
                }
            }
        };

        // Every other lane still waiting for this same sha is a merge arriving
        // from the side: it collapses into `lane` rather than staying open.
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for (i, slot) in lanes.iter_mut().enumerate() {
            if i != lane && slot.as_deref() == Some(sha) {
                *slot = None;
                edges.push((i, lane));
            }
        }

        let through: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(i, slot)| *i != lane && slot.is_some())
            .map(|(i, _)| i)
            .collect();

        // The first parent continues in this lane; the rest branch out.
        lanes[lane] = None;
        for (n, parent) in parents.iter().enumerate() {
            let parent = parent.as_ref();
            let existing = lanes.iter().position(|l| l.as_deref() == Some(parent));
            let target = if let (0, Some(i)) = (n, existing) {
                // The first parent keeps the straight line. If another lane was
                // already waiting for it, the two collapse into the leftmost so
                // the main line does not drift sideways down the graph.
                let keep = i.min(lane);
                let drop = i.max(lane);
                lanes[drop] = None;
                lanes[keep] = Some(parent.to_string());
                keep
            } else if let Some(i) = existing {
                i
            } else if n == 0 {
                lanes[lane] = Some(parent.to_string());
                lane
            } else {
                let free = lanes.iter().position(Option::is_none);
                let i = match free {
                    Some(i) => i,
                    None => {
                        lanes.push(None);
                        lanes.len() - 1
                    }
                };
                lanes[i] = Some(parent.to_string());
                i
            };
            edges.push((lane, target));
        }

        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }

        let width = lanes.len().max(lane + 1).max(
            edges.iter().map(|(a, b)| a.max(b) + 1).max().unwrap_or(0),
        );
        out.push(Node { lane, edges, through, width });
    }
    out
}
