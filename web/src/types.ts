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

export interface Line {
  kind: "context" | "add" | "del";
  old_no?: number;
  new_no?: number;
  content: string;
  no_newline?: boolean;
  words?: Range[];
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

export interface Head {
  branch: string; sha: string; subject: string; detached: boolean;
}

export interface Commit {
  sha: string; author: string; date: string; subject: string; body: string;
  merge: boolean;
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
}

export interface FileList {
  files: FileEntry[];
  stats: { files: number; additions: number; deletions: number };
}
