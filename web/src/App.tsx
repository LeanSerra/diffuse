import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCommits, getFiles, getSession, keepAlive } from "./api";
import { CommitGraph } from "./components/CommitGraph";
import { FileCard } from "./components/FileCard";
import { Sidebar } from "./components/Sidebar";
import type { CommitPage, CommitRange, FileList, Session } from "./types";

const SIDEBAR_KEY = "diffuse:sidebar";

function storedSidebar() {
  try {
    return localStorage.getItem(SIDEBAR_KEY) !== "off";
  } catch {
    return true;
  }
}

export default function App() {
  const [session, setSession] = useState<Session | null>(null);
  const [list, setList] = useState<FileList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [current, setCurrent] = useState<string | null>(null);
  const [sidebar, setSidebar] = useState(storedSidebar);
  const [nonce, setNonce] = useState(0);
  /** null = the command diffuse was launched with; otherwise a sha or "worktree". */
  const [rev, setRev] = useState<string | null>(
    () => new URLSearchParams(location.search).get("rev"),
  );
  const [graph, setGraph] = useState(false);
  /**
   * The range belongs to the command diffuse was launched with. Reading it off
   * the current session would lose it the moment you open a commit, because a
   * commit view is a `show` and has no range — and then the graph you came from
   * would have no way back.
   */
  const [range, setRange] = useState<CommitRange | null>(null);
  const [page, setPage] = useState<CommitPage | null>(null);

  const cards = useRef(new Map<string, HTMLElement>());
  const main = useRef<HTMLElement | null>(null);
  const observer = useRef<IntersectionObserver | null>(null);
  const visible = useRef(new Set<string>());
  const order = useRef<string[]>([]);
  // While a programmatic scroll is in flight, every card it passes over would
  // otherwise claim to be the current one.
  const lockUntil = useRef(0);
  // A jump that is still being held in place while content loads above it.
  const pending = useRef<{ path: string; deadline: number } | null>(null);
  const settling = useRef(0);

  // The open stream is what keeps the diffuse process alive. Close the tab and
  // the command finishes.
  useEffect(() => {
    const es = keepAlive();
    return () => es.close();
  }, []);

  useEffect(() => {
    let live = true;
    Promise.all([getSession(rev), getFiles(rev)])
      .then(([s, f]) => {
        if (!live) return;
        setSession(s);
        setList(f);
        if (rev === null) setRange(s.range);
        setError(null);
        setCurrent(f.files[0]?.path ?? null);
      })
      .catch((e) => live && setError(String(e.message ?? e)));
    return () => { live = false; };
  }, [nonce, rev]);

  useEffect(() => {
    if (!range) return;
    let live = true;
    getCommits()
      .then((p) => live && setPage(p))
      .catch(() => {});
    return () => { live = false; };
  }, [range, nonce]);

  /**
   * What the older/newer buttons step through: the uncommitted entry, if there
   * is one, then the commits in the order the graph draws them.
   */
  const walk = useMemo(() => {
    if (!page) return [] as string[];
    const revs = page.commits.map((c) => c.sha);
    return page.range?.uncommitted ? ["worktree", ...revs] : revs;
  }, [page]);

  const paths = useMemo(() => list?.files.map((f) => f.path) ?? [], [list]);
  order.current = paths;

  /**
   * Decide which file is being read: the last one whose top has passed a
   * horizontal selection line near the top of the viewport.
   *
   * That line sweeps downward through the final screenful. Without it the last
   * files in a diff can never be selected — a short file at the end of the
   * document never gets its top above a fixed line, because there is nothing
   * below it left to scroll. Sweeping the line to the bottom edge as you reach
   * the end gives every trailing file its turn.
   */
  const recompute = useCallback(() => {
    const root = main.current;
    if (!root || Date.now() < lockUntil.current) return;
    const top = root.getBoundingClientRect().top;
    const height = root.clientHeight;
    const remaining = root.scrollHeight - root.clientHeight - root.scrollTop;
    const base = height * 0.2;
    const line =
      remaining >= height
        ? base
        : base + (height - base) * (1 - Math.max(remaining, 0) / height);

    let best: string | null = null;
    let bestAt = -1;
    let firstVisible: string | null = null;
    let firstAt = Infinity;
    for (const path of visible.current) {
      const el = cards.current.get(path);
      if (!el) continue;
      const at = order.current.indexOf(path);
      if (at === -1) continue;
      if (at < firstAt) {
        firstAt = at;
        firstVisible = path;
      }
      if (el.getBoundingClientRect().top - top <= line && at > bestAt) {
        bestAt = at;
        best = path;
      }
    }
    const pick = best ?? firstVisible;
    if (pick) setCurrent(pick);
  }, []);

  useEffect(() => {
    const root = main.current;
    if (!root) return;
    visible.current.clear();
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const path = (entry.target as HTMLElement).dataset.path;
          if (!path) continue;
          if (entry.isIntersecting) visible.current.add(path);
          else visible.current.delete(path);
        }
        recompute();
      },
      { root, threshold: 0 },
    );
    observer.current = io;
    for (const el of cards.current.values()) io.observe(el);

    // Scrolling inside one tall file changes no intersections, but it does move
    // the selection line, so the scroll position has to be consulted directly.
    let queued = false;
    const onScroll = () => {
      if (queued) return;
      queued = true;
      requestAnimationFrame(() => {
        queued = false;
        recompute();
      });
    };
    root.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      io.disconnect();
      observer.current = null;
      root.removeEventListener("scroll", onScroll);
    };
  }, [paths, recompute]);

  const registerRef = useCallback((path: string, el: HTMLElement | null) => {
    const previous = cards.current.get(path);
    if (previous && previous !== el) observer.current?.unobserve(previous);
    if (el) {
      cards.current.set(path, el);
      observer.current?.observe(el);
    } else {
      cards.current.delete(path);
      visible.current.delete(path);
    }
  }, []);

  const align = useCallback((path: string) => {
    const root = main.current;
    const el = cards.current.get(path);
    if (!root || !el) return 0;
    // Scroll by the measured delta rather than with scrollIntoView, which
    // lands a few pixels short here and leaves the header clipped.
    const delta = el.getBoundingClientRect().top - root.getBoundingClientRect().top;
    if (delta !== 0) root.scrollTo({ top: root.scrollTop + delta, behavior: "instant" });
    return delta;
  }, []);

  /**
   * Hold a jump on its target while the page settles.
   *
   * Files load as they come near the viewport, so jumping across a long diff
   * scrolls to where the target sits *now*, among placeholders that are about
   * to grow. The target then slides out from under you, and clicking again only
   * gets part of the way there. Re-aligning each frame until the layout stops
   * moving lands the first click where it was aimed.
   */
  const settle = useCallback(() => {
    cancelAnimationFrame(settling.current);
    const step = () => {
      const job = pending.current;
      if (!job) return;
      align(job.path);
      lockUntil.current = Date.now() + 200;
      if (Date.now() > job.deadline) {
        pending.current = null;
        return;
      }
      settling.current = requestAnimationFrame(step);
    };
    settling.current = requestAnimationFrame(step);
  }, [align]);

  const jump = useCallback((path: string) => {
    setCurrent(path);
    lockUntil.current = Date.now() + 400;
    align(path);
    pending.current = { path, deadline: Date.now() + 2500 };
    settle();
  }, [align, settle]);

  // Any deliberate scroll of your own releases the jump immediately, so
  // diffuse never drags you back to where it was heading.
  useEffect(() => {
    const release = () => {
      pending.current = null;
      cancelAnimationFrame(settling.current);
    };
    const opts = { passive: true } as const;
    window.addEventListener("wheel", release, opts);
    window.addEventListener("touchstart", release, opts);
    return () => {
      window.removeEventListener("wheel", release);
      window.removeEventListener("touchstart", release);
      cancelAnimationFrame(settling.current);
    };
  }, []);

  /** Apply a selection without touching history — used by Back and Forward. */
  const show = useCallback((next: string | null, withGraph: boolean) => {
    setRev(next);
    setGraph(withGraph);
    setCurrent(null);
    pending.current = null;
    main.current?.scrollTo({ top: 0, behavior: "instant" });
  }, []);

  const pick = useCallback(
    (next: string | null) => {
      show(next, false);
      // Each commit you open is a place you can come back from, so the
      // browser's Back button steps through them instead of leaving diffuse.
      history.pushState(
        { rev: next, graph: true },
        "",
        next ? `?rev=${encodeURIComponent(next)}` : location.pathname,
      );
    },
    [show],
  );

  useEffect(() => {
    const onPop = (e: PopStateEvent) => {
      const state = e.state as { rev?: string | null; graph?: boolean } | null;
      const next = state?.rev ?? new URLSearchParams(location.search).get("rev");
      show(next ?? null, state?.graph ?? false);
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [show]);

  /** Negative goes up the list (newer), positive goes down (older). */
  const step = useCallback(
    (delta: number) => {
      const at = rev ? walk.indexOf(rev) : -1;
      if (at === -1) return;
      const next = walk[at + delta];
      if (next) pick(next);
    },
    [rev, walk, pick],
  );

  const toggleSidebar = useCallback(() => {
    setSidebar((open) => {
      try {
        localStorage.setItem(SIDEBAR_KEY, open ? "off" : "on");
      } catch {
        // A viewer with site data blocked still gets the toggle, just not the memory.
      }
      return !open;
    });
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const target = e.target as HTMLElement | null;
      if (target && /^(INPUT|TEXTAREA)$/.test(target.tagName)) return;
      if (e.key === "r") { setNonce((n) => n + 1); return; }
      if (e.key === "b") { toggleSidebar(); return; }
      if (e.key === "[") { step(-1); return; }
      if (e.key === "]") { step(1); return; }
      if (e.key === "g" && range) {
        setGraph((v) => !v);
        setSidebar(true);
        return;
      }
      if (e.key !== "j" && e.key !== "k") return;
      e.preventDefault();
      const at = current ? paths.indexOf(current) : -1;
      const next = e.key === "j"
        ? Math.min(at + 1, paths.length - 1)
        : Math.max(at - 1, 0);
      if (paths[next]) jump(paths[next]);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [current, paths, jump, toggleSidebar, range, step]);

  const bar = (
    <Bar
      session={session}
      list={list}
      sidebar={sidebar}
      graph={graph}
      rev={rev}
      range={range}
      walkAt={rev ? walk.indexOf(rev) : -1}
      walkLength={walk.length}
      onStep={step}
      onToggleSidebar={toggleSidebar}
      onToggleGraph={() => { setGraph((g) => !g); setSidebar(true); }}
      onBack={() => pick(null)}
      onRefresh={() => setNonce((n) => n + 1)}
    />
  );

  if (error) {
    return (
      <div className="shell" data-side={sidebar ? "on" : "off"}>
        {bar}
        <main className="main" ref={main}><pre className="error">{error}</pre></main>
      </div>
    );
  }

  const files = list?.files ?? [];

  return (
    <div className="shell" data-side={sidebar ? "on" : "off"}>
      {bar}
      {sidebar &&
        (graph ? (
          <CommitGraph
            command={session?.command ?? "the diff"}
            page={page}
            current={rev}
            onPick={pick}
          />
        ) : (
          <Sidebar files={files} current={current} onPick={jump} />
        ))}
      <main className="main" ref={main}>
        {session?.commit && (
          <article className="commit">
            <h1>{session.commit.subject}</h1>
            <p className="meta">
              {session.commit.sha.slice(0, 12)} · {session.commit.author} ·{" "}
              {session.commit.date.slice(0, 10)}
            </p>
            {session.commit.merge && (
              <p className="merge-note">
                Merge commit — shown against its first parent, which is what the
                line counts describe.
              </p>
            )}
            {session.commit.body && <pre>{session.commit.body}</pre>}
          </article>
        )}
        {list && files.length === 0 ? (
          <p className="empty">
            <span className="mark">▚</span>
            Nothing to show.
            <br />
            <code>{session?.command}</code> came back empty.
          </p>
        ) : (
          <div className="stack">
            {files.map((f) => (
              <FileCard
                key={`${nonce}-${rev ?? ""}-${f.path}`}
                entry={f}
                rev={rev}
                current={f.path === current}
                registerRef={registerRef}
                onCollapse={() => jump(f.path)}
              />
            ))}
          </div>
        )}
      </main>
    </div>
  );
}

/**
 * A panel with its left column filled when the file list is showing and hollow
 * when it is hidden, so the icon depicts the state you are looking at.
 */
function PanelIcon({ open }: { open: boolean }) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false">
      <rect
        x="1.75" y="2.75" width="12.5" height="10.5" rx="1.75"
        fill="none" stroke="currentColor" strokeWidth="1.4"
      />
      <path
        d="M6.35 2.75V13.25"
        stroke="currentColor" strokeWidth="1.4" strokeLinecap="round"
      />
      {open && (
        <path
          d="M3.5 3.75h2.85v8.5H3.5A0.75 0.75 0 0 1 2.75 11.5v-6.75A0.75 0.75 0 0 1 3.5 3.75z"
          fill="currentColor"
        />
      )}
    </svg>
  );
}

function Bar({
  session, list, sidebar, graph, rev, range, walkAt, walkLength, onStep, onToggleSidebar, onToggleGraph, onBack, onRefresh,
}: {
  session: Session | null;
  list: FileList | null;
  sidebar: boolean;
  graph: boolean;
  rev: string | null;
  range: CommitRange | null;
  walkAt: number;
  walkLength: number;
  onStep: (delta: number) => void;
  onToggleSidebar: () => void;
  onToggleGraph: () => void;
  onBack: () => void;
  onRefresh: () => void;
}) {
  const [verb, ...rest] = (session?.command ?? "git diff").split(" ").slice(1);
  return (
    <header className="bar">
      <button
        className="toggle"
        onClick={onToggleSidebar}
        aria-pressed={sidebar}
        title={`${sidebar ? "Hide" : "Show"} the file list (b)`}
      >
        <PanelIcon open={sidebar} />
        {list ? <span className="toggle-count">{list.stats.files}</span> : null}
      </button>
      <span className="prompt" title={session?.root ?? undefined}>
        <span className="who">diffuse</span>
        <span className="punct">@</span>
        <span className="where">{session?.name ?? "…"}</span>
        <span className="punct">$</span>
      </span>
      <span className="command">
        git <b>{verb}</b> {rest.join(" ")}
      </span>
      {rev && (
        <button className="refresh" onClick={onBack} title="Back to the whole diff">
          ← whole diff
        </button>
      )}
      {walkAt !== -1 && (
        <span className="stepper">
          <button
            className="refresh"
            onClick={() => onStep(-1)}
            disabled={walkAt === 0}
            title="The commit above this one in the graph ([)"
          >
            ↑ newer
          </button>
          <button
            className="refresh"
            onClick={() => onStep(1)}
            disabled={walkAt >= walkLength - 1}
            title="The commit below this one in the graph (])"
          >
            older ↓
          </button>
        </span>
      )}
      {range && (
        <button
          className="refresh"
          onClick={onToggleGraph}
          aria-pressed={graph}
          title="Show the commits this diff is made of (g)"
        >
          commits
        </button>
      )}
      {list && (
        <span className="bar-stats">
          <span className="add-count">+{list.stats.additions}</span>
          <span className="del-count">−{list.stats.deletions}</span>
        </span>
      )}
      <button className="refresh" onClick={onRefresh}>refresh</button>
    </header>
  );
}
