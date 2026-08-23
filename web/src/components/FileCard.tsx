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
 * Roughly how tall this file's diff will be once it arrives.
 *
 * A fixed-size placeholder makes every unloaded file three rows tall, so the
 * document is far shorter than it will end up being and any scroll past a
 * pending file lands in the wrong place. Guessing from the line counts we
 * already have keeps the document close to its real length.
 */
function estimateHeight(entry: FileEntry) {
  if (entry.binary) return 44;
  const rows = Math.min(entry.additions + entry.deletions + 6, 5000);
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
  entry, rev, current, registerRef, onCollapse,
}: {
  entry: FileEntry;
  rev: string | null;
  current: boolean;
  registerRef: (path: string, el: HTMLElement | null) => void;
  /** Bring this card's header to the top after it collapses. */
  onCollapse: () => void;
}) {
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(true);
  const [near, setNear] = useState(false);
  const [copied, setCopied] = useState(false);
  const host = useRef<HTMLElement | null>(null);
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

  useLayoutEffect(() => {
    if (!justToggled.current) return;
    justToggled.current = false;
    if (open) return;
    // One frame later, so the collapsed layout is final before measuring.
    requestAnimationFrame(onCollapse);
  }, [open, onCollapse]);

  // Bodies are fetched as the card approaches the viewport, so opening a
  // 2,000-file diff costs one request, not two thousand.
  useEffect(() => {
    const el = host.current;
    if (!el || near) return;
    const io = new IntersectionObserver(
      (entries) => entries.some((e) => e.isIntersecting) && setNear(true),
      { rootMargin: "600px 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [near]);

  useEffect(() => {
    if (!near || diff || error) return;
    let live = true;
    getFile(entry, rev)
      .then((d) => live && setDiff(d))
      .catch((e) => live && setError(String(e.message ?? e)));
    return () => { live = false; };
  }, [near, diff, error, entry, rev]);

  const force = () => {
    setDiff(null);
    getFile(entry, rev, true).then(setDiff).catch((e) => setError(String(e.message ?? e)));
  };

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

      {open && (
        <>
          {error && <p className="note">{error}</p>}
          {!error && !diff && (
            <div className="skeleton" style={{ height: estimateHeight(entry) }} />
          )}
          {!error && diff && <DiffBody diff={diff} onForce={force} />}
        </>
      )}
    </section>
  );
}
