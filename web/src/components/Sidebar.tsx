import type { FileEntry } from "../types";

/**
 * `apps/web/src/app/(dashboard)/grafo/` becomes `…/(dashboard)/grafo/`.
 * The tail of a directory is what tells two sibling files apart; its head is
 * the same for everything in the repo.
 */
function shortDir(path: string) {
  const at = path.lastIndexOf("/");
  if (at === -1) return "";
  const segments = path.slice(0, at).split("/");
  const kept = segments.slice(-2).join("/");
  return (segments.length > 2 ? "…/" : "") + kept;
}

const BADGE: Record<FileEntry["status"], string> = {
  added: "A", deleted: "D", modified: "M",
  renamed: "R", copied: "C", untracked: "U",
};

export function Sidebar({
  files, current, onPick,
}: {
  files: FileEntry[];
  current: string | null;
  onPick: (path: string) => void;
}) {
  return (
    <nav className="side" aria-label="Changed files">
      <div className="side-head">
        {files.length} {files.length === 1 ? "file" : "files"}
      </div>
      {files.map((f) => {
        const at = f.path.lastIndexOf("/");
        const dir = shortDir(f.path);
        return (
          <button
            key={f.path}
            className="file-link"
            aria-current={f.path === current}
            onClick={() => onPick(f.path)}
            title={f.path}
          >
            <span className="badge" data-s={f.status}>{BADGE[f.status]}</span>
            <span className="name">
              <span className="base">{at === -1 ? f.path : f.path.slice(at + 1)}</span>
              {dir && <span className="dir">{dir}</span>}
            </span>
            <span className="counts">
              <span className="add-count">+{f.additions}</span>{" "}
              <span className="del-count">−{f.deletions}</span>
            </span>
          </button>
        );
      })}
      <p className="keys">
        <kbd>j</kbd><kbd>k</kbd> move · <kbd>b</kbd> hide list · <kbd>r</kbd> refresh
      </p>
    </nav>
  );
}
