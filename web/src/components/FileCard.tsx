import type { CSSProperties } from "react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { getFile } from "../api";
import type { FileDiff, FileEntry } from "../types";
import { DiffBody } from "./DiffBody";

function splitPath(path: string) {
  const at = path.lastIndexOf("/");
  return at === -1
    ? { dir: "", base: path }
    : { dir: path.slice(0, at + 1), base: path.slice(at + 1) };
}

/** Row height from the stylesheet's --row, used to size placeholders. */
const ROW = 20;

/**
 * How tall this file's diff is, or will be once it arrives.
 *
 * A fixed-size placeholder makes every unloaded file three rows tall, so the
 * document is far shorter than it will end up being and any scroll past a
 * pending file lands in the wrong place.
 *
 * Once the diff is here the row count is exact, so use it. Before that all we
 * have is `git --numstat`, which counts only changed lines — the context lines
 * around them are rendered too and are invisible to it, so that guess runs
 * short by roughly the context width per hunk and the document is built too
 * small until every file has landed.
 */
function estimateHeight(entry: FileEntry, diff: FileDiff | null) {
  if (entry.binary) return 44;
  const rows = diff
    ? diff.hunks.reduce((n, h) => n + h.lines.length + 1, 0)
    : Math.min(entry.additions + entry.deletions + 6, 5000);
  return rows * ROW + 8;
}

function CopyIcon({ done }: { done: boolean }) {
  return done ? (
    <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true" focusable="false">
      <path
        d="M3.5 8.5 6.5 11.5 12.5 4.5"
        fill="none" stroke="currentColor" strokeWidth="1.6"
        strokeLinecap="round" strokeLinejoin="round"
      />
    </svg>
  ) : (
    <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true" focusable="false">
      <rect
        x="5.75" y="2.25" width="8" height="9.5" rx="1.5"
        fill="none" stroke="currentColor" strokeWidth="1.3"
      />
      <path
        d="M10.5 13.75H3.75A1.5 1.5 0 0 1 2.25 12.25V5"
        fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round"
      />
    </svg>
  );
}

const CHIP: Partial<Record<FileEntry["status"], string>> = {
  added: "added",
  deleted: "deleted",
  renamed: "renamed",
  copied: "copied",
  untracked: "untracked",
};

export function FileCard({
  entry, rev, current, registerRef, onCollapse, preloaded, selfLoad,
}: {
  entry: FileEntry;
  rev: string | null;
  current: boolean;
  registerRef: (path: string, el: HTMLElement | null) => void;
  /** Bring this card's header to the top after it collapses. */
  onCollapse: () => void;
  /** This file's diff from the whole-diff stream, once it has arrived. */
  preloaded: FileDiff | null;
  /** The diff was past the server's ceiling: fetch this file on approach. */
  selfLoad: boolean;
}) {
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(true);
  const [near, setNear] = useState(false);
  const [copied, setCopied] = useState(false);
  const host = useRef<HTMLElement | null>(null);
  const body = useRef<HTMLDivElement | null>(null);
  const justToggled = useRef(false);
  const copiedFor = useRef(0);

  const copyPath = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(entry.path);
    } catch {
      // The Clipboard API needs a secure context. Loopback counts as one, but
      // fall back rather than silently doing nothing if it is refused.
      const box = document.createElement("textarea");
      box.value = entry.path;
      box.style.cssText = "position:fixed;top:0;left:0;opacity:0";
      document.body.appendChild(box);
      box.select();
      try {
        document.execCommand("copy");
      } catch {
        box.remove();
        return;
      }
      box.remove();
    }
    setCopied(true);
    window.clearTimeout(copiedFor.current);
    copiedFor.current = window.setTimeout(() => setCopied(false), 1400);
  }, [entry.path]);

  useEffect(() => () => window.clearTimeout(copiedFor.current), []);

  /**
   * Collapse only moves the page when you were reading inside the file.
   *
   * If its header has scrolled off the top, the rows you were looking at are
   * about to vanish and be replaced by some other file's middle, so the header
   * is brought up to where you were already looking. If the header is on screen
   * — which is the usual case, since you clicked it — nothing above you changes
   * height and the page should stay exactly where it is.
   */
  const toggleOpen = useCallback(() => {
    const el = host.current;
    const scroller = el?.closest(".main") as HTMLElement | null;
    const above =
      el && scroller
        ? el.getBoundingClientRect().top - scroller.getBoundingClientRect().top < -1
        : false;
    justToggled.current = above;
    setOpen((v) => !v);
  }, []);

  /*
   * `hidden="until-found"` is set on the node rather than rendered, because
   * React treats `hidden` as a boolean and would write `hidden=""`, which
   * hides the content outright and puts it back out of find's reach.
   */
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    if (open) el.removeAttribute("hidden");
    else el.setAttribute("hidden", "until-found");
  }, [open]);

  // The browser expands a collapsed card by itself when a search lands inside
  // it. This is how the chevron finds out, so it never claims the file is
  // still collapsed while you are reading it.
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    const reveal = () => setOpen(true);
    el.addEventListener("beforematch", reveal as EventListener);
    return () => el.removeEventListener("beforematch", reveal as EventListener);
  }, []);

  useLayoutEffect(() => {
    if (!justToggled.current) return;
    justToggled.current = false;
    if (open) return;
    // One frame later, so the collapsed layout is final before measuring.
    requestAnimationFrame(onCollapse);
  }, [open, onCollapse]);

  // Only reached past the server's ceiling. Normally every file arrives on
  // the whole-diff stream, because find-in-page cannot reach text that is not
  // in the page — a card that waits for you to scroll to it is a card the
  // browser's own search will never see.
  useEffect(() => {
    const el = host.current;
    if (!el || near || !selfLoad) return;
    const io = new IntersectionObserver(
      (entries) => entries.some((e) => e.isIntersecting) && setNear(true),
      { rootMargin: "600px 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [near, selfLoad]);

  useEffect(() => {
    if (!near || !selfLoad || diff || error) return;
    let live = true;
    getFile(entry, rev)
      .then((d) => live && setDiff(d))
      .catch((e) => live && setError(String(e.message ?? e)));
    return () => { live = false; };
  }, [near, selfLoad, diff, error, entry, rev]);

  const force = () => {
    setDiff(null);
    getFile(entry, rev, true).then(setDiff).catch((e) => setError(String(e.message ?? e)));
  };

  // The streamed copy is authoritative; `diff` only ever holds a file this
  // card fetched itself, which happens past the ceiling or on a forced reload.
  const shown = diff ?? preloaded;

  const { dir, base } = splitPath(entry.path);
  const chip = CHIP[entry.status];

  return (
    <section
      className="card"
      data-current={current}
      data-path={entry.path}
      id={`f-${entry.path}`}
      ref={(el) => { host.current = el; registerRef(entry.path, el); }}
    >
      <header className="card-head">
        <button
          className="collapse"
          onClick={toggleOpen}
          aria-expanded={open}
          aria-label={open ? `Collapse ${entry.path}` : `Expand ${entry.path}`}
        >
          {open ? "▾" : "▸"}
        </button>
        <span className="named">
          <span className="path">
            {entry.old_path && <span className="from">{entry.old_path} → </span>}
            <span className="dir">{dir}</span>
            <span className="base">{base}</span>
          </span>
          <button
            className="copy"
            data-done={copied}
            onClick={copyPath}
            title={copied ? "Path copied" : "Copy path"}
            aria-label={copied ? `Copied ${entry.path}` : `Copy path ${entry.path}`}
          >
            <CopyIcon done={copied} />
          </button>
        </span>
        <span className="head-gap" />
        {chip && <span className="chip">{chip}</span>}
        <span className="add-count">+{entry.additions}</span>
        <span className="del-count">−{entry.deletions}</span>
      </header>

      {/*
        Collapsed content stays in the DOM, hidden, rather than being unmounted:
        `until-found` lets the browser reveal it when a search lands inside, so
        collapsing a file does not put it out of reach of Ctrl+F.
      */}
      <div
        ref={body}
        className="card-body"
        // Feeds `contain-intrinsic-size`, so a card the browser has not laid
        // out yet still takes up roughly the room it eventually will.
        style={{ "--est": `${estimateHeight(entry, shown)}px` } as CSSProperties}
      >
        {error && <p className="note">{error}</p>}
        {!error && !shown && (
          <div className="skeleton" style={{ height: estimateHeight(entry, null) }} />
        )}
        {!error && shown && <DiffBody diff={shown} onForce={force} />}
      </div>
    </section>
  );
}
