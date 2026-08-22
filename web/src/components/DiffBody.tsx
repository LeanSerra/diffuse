import type { ReactNode } from "react";
import type { FileDiff, Hunk, Line, Range } from "../types";

const SIGIL = { add: "+", del: "-", context: " " } as const;

/** Word ranges arrive as UTF-16 offsets, which is exactly what slice() wants. */
function content(line: Line) {
  const { content: text, words } = line;
  if (!words || words.length === 0) return text;
  const out: ReactNode[] = [];
  let at = 0;
  words.forEach((w: Range, i) => {
    if (w.start > at) out.push(text.slice(at, w.start));
    out.push(<mark key={i}>{text.slice(w.start, w.end)}</mark>);
    at = w.end;
  });
  if (at < text.length) out.push(text.slice(at));
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
