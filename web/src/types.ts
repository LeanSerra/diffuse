export type Status =
  | "added" | "deleted" | "modified" | "renamed" | "copied" | "untracked";

export interface FileEntry {
  path: string;
  old_path?: string;
  status: Status;
  additions: number;
  deletions: number;
  binary: boolean;
  untracked: boolean;
}

export interface Range { start: number; end: number }

/** A run of characters sharing one syntax class, in UTF-16 offsets. */
export interface Span { start: number; end: number; class: string }

export interface Line {
  kind: "context" | "add" | "del";
  old_no?: number;
  new_no?: number;
  content: string;
  no_newline?: boolean;
  words?: Range[];
  syntax?: Span[];
}

export interface Hunk {
  header: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  section?: string;
  lines: Line[];
}

export interface FileDiff {
  path: string;
  old_path?: string;
  hunks: Hunk[];
  binary: boolean;
  combined: boolean;
  old_mode?: string;
  new_mode?: string;
  truncated: boolean;
}

/** The stream's opening record: how many files follow, and whether any will. */
export interface StreamBegin {
  type: "begin";
  files: number;
  /** False when the diff is past the server's ceiling and nothing follows. */
  inline: boolean;
  lines: number;
  cap: number;
}

export type StreamRecord =
  | StreamBegin
  | { type: "file"; diff: FileDiff }
  | { type: "end" }
  | { type: "error"; error: string };

export interface Head {
  branch: string; sha: string; subject: string; detached: boolean;
}

export interface Commit {
  sha: string; author: string; date: string; subject: string; body: string;
  merge: boolean;
}

export interface CommitRange {
  spec: string;
  /** Commits the other side has that this one does not; the aggregate reverses them. */
  behind: number;
  /** The revision named on the command line, when it has moved ahead. */
  other: string | null;
  uncommitted: boolean;
}

export interface Commit {
  sha: string; short: string; parents: string[];
  author: string; date: string; subject: string; refs: string[];
}

/** Lane geometry for one row of the graph. */
export interface GraphNode {
  lane: number;
  edges: [number, number][];
  through: number[];
  width: number;
}

export interface CommitPage {
  range: CommitRange | null;
  commits: Commit[];
  graph: GraphNode[];
  hasMore: boolean;
}

export interface Session {
  command: string;
  root: string;
  name: string | null;
  head: Head | null;
  commit: Commit | null;
  worktreeRight: boolean;
  ignoredFlags: string[];
  subcommand: string;
  range: CommitRange | null;
}

export interface FileList {
  files: FileEntry[];
  stats: { files: number; additions: number; deletions: number };
}
