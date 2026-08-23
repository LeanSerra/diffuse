import type { ReactNode } from "react";
import type { FileDiff, Hunk, Line, Range, Span } from "../types";

const SIGIL = { add: "+", del: "-", context: " " } as const;

function covering<T extends { start: number; end: number }>(ranges: T[] | undefined, at: number) {
  return ranges?.find((r) => r.start <= at && at < r.end);
}

/**
 * Syntax classes and word-level ranges both decorate the same characters, so
 * the line is cut at every boundary either of them introduces. Syntax owns the
 * colour, the word range owns the background, and a segment inside both keeps
 * both. All offsets are UTF-16 code units, which is exactly what slice() wants.
 */
function content(line: Line) {
  const { content: text, words, syntax } = line;
  if (!words?.length && !syntax?.length) return text;

  const length = text.length;
  const edges = new Set<number>([0, length]);
  for (const s of syntax ?? []) { edges.add(s.start); edges.add(s.end); }
  for (const w of words ?? []) { edges.add(w.start); edges.add(w.end); }
  const cuts = [...edges].filter((n) => n >= 0 && n <= length).sort((a, b) => a - b);

  const out: ReactNode[] = [];
  for (let i = 0; i < cuts.length - 1; i++) {
    const [from, to] = [cuts[i], cuts[i + 1]];
    if (from === to) continue;
    const piece = text.slice(from, to);
    const token = covering<Span>(syntax, from);
    const changed = Boolean(covering<Range>(words, from));
    const names = [token && `t-${token.class}`, changed && "w"].filter(Boolean).join(" ");
    out.push(names ? <span className={names} key={i}>{piece}</span> : piece);
  }
  return out;
}

function Rows({ hunk }: { hunk: Hunk }) {
  return (
    <>
      <div className="hunk-head">
        <span className="nums">@@</span>
        <span className="section">
          {hunk.section ?? `${hunk.old_start},${hunk.old_lines} → ${hunk.new_start},${hunk.new_lines}`}
        </span>
      </div>
      {hunk.lines.map((line, i) => (
        <div className="row" data-k={line.kind} key={i}>
          <span className="n">{line.old_no ?? ""}</span>
          <span className="n">{line.new_no ?? ""}</span>
          <span className="c" data-sigil={SIGIL[line.kind]}>
            {content(line)}
            {line.no_newline && <span className="eof">no newline at end of file</span>}
          </span>
        </div>
      ))}
    </>
  );
}

export function DiffBody({ diff, onForce }: { diff: FileDiff; onForce: () => void }) {
  if (diff.binary) {
    return <p className="note">Binary file. diffuse renders text patches only.</p>;
  }
  if (diff.truncated) {
    return (
      <div className="note">
        This file is large enough that rendering it will be slow.
        <button onClick={onForce}>Render it anyway</button>
      </div>
    );
  }
  if (diff.hunks.length === 0) {
    return <p className="note">No content changed. Only the file's mode or name differs.</p>;
  }
  return (
    <div className="body">
      {diff.combined && (
        <p className="note">
          Combined diff from a merge commit. Line numbers on the left are omitted
          because each removal belongs to a different parent.
        </p>
      )}
      {diff.hunks.map((h, i) => <Rows hunk={h} key={i} />)}
    </div>
  );
}
