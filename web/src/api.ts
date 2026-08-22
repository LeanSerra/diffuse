import type { FileDiff, FileEntry, FileList, Session } from "./types";

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
    history.replaceState(null, "", location.pathname);
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

export const getSession = () => get<Session>("/api/session");
export const getFiles = () => get<FileList>("/api/files");

export function getFile(entry: FileEntry, force = false): Promise<FileDiff> {
  const params: Record<string, string> = { path: entry.path };
  if (entry.old_path) params.old = entry.old_path;
  if (entry.untracked) params.untracked = "1";
  if (force) params.force = "1";
  return get<FileDiff>("/api/file", params);
}

/// Holding this stream open is what keeps the diffuse process alive; when the
/// last tab closes, the command finishes.
export function keepAlive(): EventSource {
  return new EventSource(`/api/events?t=${encodeURIComponent(token)}`);
}
