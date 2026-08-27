import type {
  CommitPage, FileDiff, FileEntry, FileList, Session, StreamBegin, StreamRecord,
} from "./types";

/**
 * The token is handed to the page in its launch URL. It is moved into
 * sessionStorage and stripped from the address bar: keeping it only in memory
 * would make a plain page reload fatal, since the reloaded page would have
 * neither the query parameter nor any way to ask for it again.
 *
 * sessionStorage is scoped to this tab and to this origin, and every launch of
 * diffuse gets a fresh random port, so the value cannot outlive the command
 * that issued it in any useful way.
 */
const TOKEN_KEY = "diffuse:token";

function resolveToken(): string {
  const fromUrl = new URLSearchParams(location.search).get("t");
  if (fromUrl) {
    try {
      sessionStorage.setItem(TOKEN_KEY, fromUrl);
    } catch {
      // Storage blocked: the token still works for this page load.
    }
    // Keep everything except the token, so a `?rev=` deep link survives.
    const rest = new URLSearchParams(location.search);
    rest.delete("t");
    const query = rest.toString();
    history.replaceState(null, "", location.pathname + (query ? `?${query}` : ""));
    return fromUrl;
  }
  try {
    return sessionStorage.getItem(TOKEN_KEY) ?? "";
  } catch {
    return "";
  }
}

const token = resolveToken();

async function get<T>(path: string, params: Record<string, string> = {}): Promise<T> {
  const url = new URL(path, location.origin);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  const res = await fetch(url, { headers: { "X-Diffuse-Token": token } });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body || `${res.status} ${res.statusText}`);
  }
  return res.json() as Promise<T>;
}

/** `rev` is a commit sha, "worktree", or null for the launched command. */
export const getSession = (rev: string | null) =>
  get<Session>("/api/session", rev ? { rev } : {});
export const getFiles = (rev: string | null) =>
  get<FileList>("/api/files", rev ? { rev } : {});
export const getCommits = (skip = 0) =>
  get<CommitPage>("/api/commits", { skip: String(skip) });

export function getFile(entry: FileEntry, rev: string | null, force = false): Promise<FileDiff> {
  const params: Record<string, string> = { path: entry.path };
  if (rev) params.rev = rev;
  if (entry.old_path) params.old = entry.old_path;
  if (entry.untracked) params.untracked = "1";
  if (force) params.force = "1";
  return get<FileDiff>("/api/file", params);
}

/**
 * The whole diff, one NDJSON record per file, consumed as it arrives.
 *
 * Everything has to be in the page for the browser's own find to reach a file
 * you have not scrolled to, and it is streamed rather than fetched whole so
 * cards fill in as records land instead of waiting on the slowest file.
 *
 * The server refuses very large diffs: `begin.inline` false means nothing more
 * follows and the caller should fall back to loading files one at a time.
 */
export async function streamAll(
  rev: string | null,
  on: { begin: (b: StreamBegin) => void; file: (d: FileDiff) => void; done: () => void },
  signal: AbortSignal,
): Promise<void> {
  const url = new URL("/api/all", location.origin);
  if (rev) url.searchParams.set("rev", rev);
  const res = await fetch(url, { headers: { "X-Diffuse-Token": token }, signal });
  if (!res.ok || !res.body) throw new Error(await res.text());

  const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
  // Records are newline-delimited, but a chunk can split one anywhere, so the
  // tail of each chunk is carried forward rather than parsed.
  let rest = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    rest += value;
    let at: number;
    while ((at = rest.indexOf("\n")) !== -1) {
      const line = rest.slice(0, at);
      rest = rest.slice(at + 1);
      if (!line) continue;
      const rec = JSON.parse(line) as StreamRecord;
      if (rec.type === "begin") on.begin(rec);
      else if (rec.type === "file") on.file(rec.diff);
      else if (rec.type === "error") throw new Error(rec.error);
    }
  }
  on.done();
}

/// Holding this stream open is what keeps the diffuse process alive; when the
/// last tab closes, the command finishes.
export function keepAlive(): EventSource {
  return new EventSource(`/api/events?t=${encodeURIComponent(token)}`);
}
