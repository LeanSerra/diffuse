import type { Commit, CommitPage, GraphNode } from "../types";

const LANE = 14;
const ROW = 38;

/** One row's lines: what enters from above, what leaves below. */
function Lanes({
  node, prev, width, last, pendingAbove,
}: {
  node: GraphNode;
  /** The row above, so this one can meet the line it left hanging. */
  prev: GraphNode | null;
  width: number;
  /** The oldest row, with nothing below it: its parents are not drawn. */
  last: boolean;
  /** The uncommitted row sits above, joined by a dashed line. */
  pendingAbove: boolean;
}) {
  const x = (lane: number) => lane * LANE + LANE / 2;
  const mid = ROW / 2;
  const incoming = node.edges.filter(([from, to]) => to === node.lane && from !== node.lane);
  // Nothing below the oldest row is drawn — not its parents, not whatever the
  // lanes beside it are still waiting for — so on that row every line stops at
  // the dot's own level rather than running off the bottom into nothing.
  const outgoing = last ? [] : node.edges.filter(([from]) => from === node.lane);
  // An edge arriving straight down this commit's own lane has the same lane at
  // both ends, so it is in neither list above. Without drawing it here the row
  // above ends at its own bottom edge and every dot floats unattached.
  const joined = prev
    ? prev.edges.some(([, to]) => to === node.lane) || prev.through.includes(node.lane)
    : pendingAbove;
  return (
    <svg className="lanes" width={width * LANE} height={ROW} aria-hidden="true">
      {node.through.map((lane) => (
        <line key={`t${lane}`} x1={x(lane)} y1={0} x2={x(lane)} y2={last ? mid : ROW} />
      ))}
      {joined && (
        <line
          className={prev ? undefined : "pending-line"}
          x1={x(node.lane)}
          y1={0}
          x2={x(node.lane)}
          y2={mid}
        />
      )}
      {incoming.map(([from], i) => (
        <path key={`i${i}`} d={`M${x(from)},0 C${x(from)},${mid * 0.7} ${x(node.lane)},${mid * 0.4} ${x(node.lane)},${mid}`} />
      ))}
      {outgoing.map(([, to], i) => (
        <path key={`o${i}`} d={`M${x(node.lane)},${mid} C${x(node.lane)},${mid + mid * 0.4} ${x(to)},${mid + mid * 0.7} ${x(to)},${ROW}`} />
      ))}
      <circle className="dot" cx={x(node.lane)} cy={mid} r={4} />
    </svg>
  );
}

export function CommitGraph({
  page, current, onPick,
}: {
  page: CommitPage | null;
  current: string | null;
  onPick: (rev: string | null) => void;
}) {
  if (!page) return <nav className="side"><div className="side-head">loading commits</div></nav>;

  const width = Math.max(1, ...page.graph.map((g) => g.width));

  return (
    <nav className="side" aria-label="Commits">
      <div className="side-head">
        {page.commits.length} {page.commits.length === 1 ? "commit" : "commits"}
      </div>

      {page.range?.uncommitted && (
        <button
          className="commit-row pending"
          aria-current={current === "worktree"}
          onClick={() => onPick("worktree")}
          title="Everything not yet committed, on top of the newest commit"
        >
          {/* A real node on the graph, joined to the commit below by a dashed
              line: this work sits on top of it but is not a commit yet. */}
          <svg className="lanes" width={width * LANE} height={ROW} aria-hidden="true">
            <line
              className="pending-line"
              x1={(page.graph[0]?.lane ?? 0) * LANE + LANE / 2}
              y1={ROW / 2}
              x2={(page.graph[0]?.lane ?? 0) * LANE + LANE / 2}
              y2={ROW}
            />
            <circle
              className="pending-dot"
              cx={(page.graph[0]?.lane ?? 0) * LANE + LANE / 2}
              cy={ROW / 2}
              r={4}
            />
          </svg>
          <span className="commit-text">
            <span className="commit-sub">Uncommitted changes</span>
            <span className="commit-meta">working tree · not a commit yet</span>
          </span>
        </button>
      )}

      {page.commits.map((c: Commit, i) => (
        <button
          key={c.sha}
          className="commit-row"
          aria-current={current === c.sha}
          onClick={() => onPick(c.sha)}
          title={`${c.short} · ${c.author} · ${c.date.slice(0, 10)}`}
        >
          <Lanes
            node={page.graph[i]}
            prev={i > 0 ? page.graph[i - 1] : null}
            width={width}
            last={i === page.commits.length - 1 && !page.hasMore}
            pendingAbove={!!page.range?.uncommitted}
          />
          <span className="commit-text">
            <span className="commit-sub">{c.subject}</span>
            <span className="commit-meta">
              {c.short} · {c.author}
              {c.refs.length > 0 && <span className="commit-refs"> {c.refs.join(" ")}</span>}
            </span>
          </span>
        </button>
      ))}

      {page.range && page.range.behind > 0 && (
        <div className="graph-note">
          <p>
            <b>{page.range.other ?? "The other side"}</b> has moved on: it has{" "}
            {page.range.behind}{" "}
            {page.range.behind === 1 ? "commit" : "commits"} this branch does not.
          </p>
          <p>
            Comparing against its <em>tip</em> means{" "}
            {page.range.behind === 1 ? "that commit shows" : "those commits show"}{" "}
            up in the diff as deletions — this branch simply does not have{" "}
            {page.range.behind === 1 ? "it" : "them"} yet. The commits below are
            only what this branch added.
          </p>
          {page.range.other && (
            <p>
              To compare against the point the branches split instead, run{" "}
              <code>diffuse diff {page.range.other}...HEAD</code>
            </p>
          )}
        </div>
      )}
      {page.hasMore && <p className="graph-note">Showing the newest {page.commits.length}.</p>}
    </nav>
  );
}
