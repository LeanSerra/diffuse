//! The local HTTP server.
//!
//! diffuse serves your source code over a loopback port, which any page in
//! your browser could otherwise read. Four things stop that: loopback-only
//! binding, an ephemeral port, a per-launch token, and a Host/Origin check
//! against DNS rebinding. All of them are here from the start because
//! retrofitting them means touching every route.

use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rayon::prelude::*;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::{IntervalStream, ReceiverStream};
use tokio_stream::StreamExt;

use crate::git::Runner;
use crate::model::FileDiff;
use crate::{highlight, words};

/// Files larger than this are not sent until the reader explicitly asks. Only
/// the per-file route applies it; the whole-diff stream exists precisely to
/// put everything in the page, so it holds nothing back.
const LINE_CEILING: usize = 5000;
/// Above this many changed lines the whole diff is not sent up front, because
/// the page would hold a DOM row per line. `--inline-all` overrides it.
const INLINE_LINE_CAP: usize = 50_000;
/// A ceiling on whole-file text highlighted for one stream. Highlighting needs
/// entire files, so a one-line change in a huge file costs the whole file;
/// past this budget the remaining files stream plain but still searchable.
const HIGHLIGHT_LINE_BUDGET: usize = 400_000;
/// How long the process lingers with no browser attached.
const IDLE_GRACE: Duration = Duration::from_secs(30);

#[derive(rust_embed::Embed)]
#[folder = "web/dist"]
struct Assets;

#[derive(Clone)]
pub struct App {
    runner: Arc<Runner>,
    token: Arc<String>,
    clients: Arc<AtomicUsize>,
    ever_connected: Arc<AtomicUsize>,
}

fn unauthorized() -> Response {
    (StatusCode::FORBIDDEN, "diffuse: bad or missing token").into_response()
}

/// A page served from an attacker's domain that resolves to 127.0.0.1 would
/// carry its own hostname here.
fn host_is_loopback(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    name == "127.0.0.1" || name == "localhost" || name == "[::1]"
}

fn authorized(app: &App, headers: &HeaderMap, q: Option<&str>) -> bool {
    if !host_is_loopback(headers) {
        return false;
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let ok = origin.starts_with("http://127.0.0.1:")
            || origin.starts_with("http://localhost:")
            || origin.starts_with("http://[::1]:");
        if !ok {
            return false;
        }
    }
    let supplied = headers
        .get("x-diffuse-token")
        .and_then(|v| v.to_str().ok())
        .or(q);
    supplied == Some(app.token.as_str())
}

#[derive(Deserialize)]
struct Auth {
    rev: Option<String>,
    t: Option<String>,
}

#[derive(Deserialize)]
struct CommitsQuery {
    skip: Option<usize>,
    limit: Option<usize>,
    t: Option<String>,
}

/// `rev` selects what to diff: a commit sha, or `worktree` for uncommitted
/// work. Absent means the command diffuse was launched with.
fn runner_for(app: &App, rev: Option<&str>) -> std::sync::Arc<Runner> {
    match rev {
        None => app.runner.clone(),
        Some("worktree") => std::sync::Arc::new(app.runner.for_worktree()),
        Some(sha) => std::sync::Arc::new(app.runner.for_commit(sha)),
    }
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
    rev: Option<String>,
    old: Option<String>,
    untracked: Option<u8>,
    force: Option<u8>,
    t: Option<String>,
}

async fn session(State(app): State<App>, headers: HeaderMap, Query(q): Query<Auth>) -> Response {
    if !authorized(&app, &headers, q.t.as_deref()) {
        return unauthorized();
    }
    let r = runner_for(&app, q.rev.as_deref());
    let body = tokio::task::spawn_blocking(move || {
        serde_json::json!({
            "command": r.inv.display_command(),
            "root": r.repo.root,
            "name": r.repo.root.file_name().map(|s| s.to_string_lossy().into_owned()),
            "head": r.head(),
            "commit": r.commit_meta(),
            "worktreeRight": r.worktree_is_right_side(),
            "ignoredFlags": r.inv.ignored,
            "subcommand": r.inv.subcommand.as_str(),
            "range": r.range(),
        })
    })
    .await
    .unwrap();
    Json(body).into_response()
}

async fn files(State(app): State<App>, headers: HeaderMap, Query(q): Query<Auth>) -> Response {
    if !authorized(&app, &headers, q.t.as_deref()) {
        return unauthorized();
    }
    let r = runner_for(&app, q.rev.as_deref());
    match tokio::task::spawn_blocking(move || r.file_list())
        .await
        .unwrap()
    {
        Ok(files) => {
            let additions: u32 = files.iter().map(|f| f.additions).sum();
            let deletions: u32 = files.iter().map(|f| f.deletions).sum();
            Json(serde_json::json!({
                "files": files,
                "stats": { "files": files.len(), "additions": additions, "deletions": deletions },
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn file(State(app): State<App>, headers: HeaderMap, Query(q): Query<FileQuery>) -> Response {
    if !authorized(&app, &headers, q.t.as_deref()) {
        return unauthorized();
    }
    let r = runner_for(&app, q.rev.as_deref());
    let path = q.path.clone();
    let force = q.force == Some(1);
    let untracked = q.untracked == Some(1);

    // Everything here shells out to git and parses, so it all belongs off the
    // async runtime — highlighting a large file is not a quick call.
    let result = tokio::task::spawn_blocking(move || {
        let parsed = if untracked {
            r.untracked_diff(&q.path).map(|d| vec![d])
        } else {
            r.patch_for(&q.path, q.old.as_deref())
                .map(|text| crate::parse::parse_patch(&text))
                .map(|mut files| {
                    // git may report several files when rename detection pairs
                    // this path with another; keep the one that was asked for.
                    files.retain(|f| {
                        f.path == q.path || f.old_path.as_deref() == Some(q.path.as_str())
                    });
                    files
                })
        };

        parsed.map(|mut files| {
            for f in files.iter_mut() {
                let size: usize = f.hunks.iter().map(|h| h.lines.len()).sum();
                if size > LINE_CEILING && !force {
                    f.truncated = true;
                    f.hunks.clear();
                    continue;
                }
                if untracked {
                    // Nothing existed before, so the file itself is the new side.
                    let text = r.read_untracked(&f.path).ok();
                    dress(f, None, text.as_deref());
                } else {
                    match r.full_text(&f.path, f.old_path.as_deref()) {
                        Some((old, new)) => dress(f, Some(&old), Some(&new)),
                        None => dress(f, None, None),
                    }
                }
            }
            files
        })
    })
    .await
    .unwrap();

    match result {
        Ok(files) => {
            let empty = FileDiff {
                path: path.clone(),
                ..Default::default()
            };
            Json(files.into_iter().next().unwrap_or(empty)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Word-level diffing and syntax highlighting, the two passes that turn a
/// parsed patch into what the client draws. Both routes go through here so a
/// file looks the same however it was asked for; passing no text skips
/// highlighting and leaves the rows plain.
fn dress(f: &mut FileDiff, old: Option<&str>, new: Option<&str>) {
    words::annotate(&mut f.hunks);
    if old.is_some() || new.is_some() {
        highlight::annotate(f, old, new);
    }
}

/// The whole diff, one NDJSON record per file.
///
/// Sending everything up front is what lets the browser's own find reach a
/// file you have not scrolled to yet. It is streamed rather than returned
/// whole so the page fills in as records land instead of waiting on the
/// slowest file.
async fn all(State(app): State<App>, headers: HeaderMap, Query(q): Query<Auth>) -> Response {
    if !authorized(&app, &headers, q.t.as_deref()) {
        return unauthorized();
    }
    let r = runner_for(&app, q.rev.as_deref());
    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(8);
    tokio::task::spawn_blocking(move || stream_all(&r, &tx));
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        // Records are only useful as they arrive; a proxy holding them back to
        // buffer the whole body would defeat the point.
        .header("x-content-type-options", "nosniff")
        .body(Body::from_stream(ReceiverStream::new(rx)))
        .unwrap()
}

fn stream_all(r: &Runner, tx: &mpsc::Sender<Result<Bytes, Infallible>>) {
    let send = |v: serde_json::Value| -> bool {
        let Ok(mut line) = serde_json::to_vec(&v) else {
            return false;
        };
        line.push(b'\n');
        tx.blocking_send(Ok(Bytes::from(line))).is_ok()
    };

    let entries = match r.file_list() {
        Ok(e) => e,
        Err(e) => {
            send(serde_json::json!({ "type": "error", "error": e.to_string() }));
            return;
        }
    };

    // Decided from the stat output, before any real work: the cost we are
    // guarding against is a DOM row per changed line, and numstat already
    // counts those.
    let lines: usize = entries
        .iter()
        .map(|f| (f.additions + f.deletions) as usize)
        .sum();
    let inline = r.inv.inline_all || lines <= INLINE_LINE_CAP;
    if !send(serde_json::json!({
        "type": "begin",
        "files": entries.len(),
        "inline": inline,
        "lines": lines,
        "cap": INLINE_LINE_CAP,
    })) {
        return;
    }
    if !inline {
        return;
    }

    let mut files = match r.whole_patch() {
        Ok(text) => crate::parse::parse_patch(&text),
        Err(e) => {
            send(serde_json::json!({ "type": "error", "error": e.to_string() }));
            return;
        }
    };
    // Untracked files appear in no git diff, so their patches are synthesized
    // and appended. Their text comes off disk rather than out of `full`.
    let untracked: std::collections::HashSet<&str> = entries
        .iter()
        .filter(|e| e.untracked)
        .map(|e| e.path.as_str())
        .collect();
    for path in &untracked {
        if let Ok(d) = r.untracked_diff(path) {
            files.push(d);
        }
    }

    let full = r.whole_full_text();

    // Spend the highlighting budget in list order, so which files come out
    // coloured does not change from run to run.
    let mut left = HIGHLIGHT_LINE_BUDGET;
    let plan: Vec<bool> = files
        .iter()
        .map(|f| {
            let cost = match full.get(&f.path) {
                Some((_, new)) => new.lines().count(),
                // Not in the diff: an untracked file, whose whole body is new.
                None => f.hunks.iter().map(|h| h.lines.len()).sum(),
            };
            if cost <= left {
                left -= cost;
                true
            } else {
                false
            }
        })
        .collect();

    // Highlighting one file knows nothing about any other, so this fans out
    // across cores; records are sent as each finishes, in whatever order.
    files.into_par_iter().zip(plan).for_each(|(mut f, colour)| {
        if !colour {
            dress(&mut f, None, None);
        } else if untracked.contains(f.path.as_str()) {
            // Nothing existed before, so the file itself is the new side.
            let text = r.read_untracked(&f.path).ok();
            dress(&mut f, None, text.as_deref());
        } else {
            match full.get(&f.path) {
                Some((old, new)) => dress(&mut f, Some(old), Some(new)),
                None => dress(&mut f, None, None),
            }
        }
        send(serde_json::json!({ "type": "file", "diff": f }));
    });

    send(serde_json::json!({ "type": "end" }));
}

async fn commits(
    State(app): State<App>,
    headers: HeaderMap,
    Query(q): Query<CommitsQuery>,
) -> Response {
    if !authorized(&app, &headers, q.t.as_deref()) {
        return unauthorized();
    }
    let r = app.runner.clone();
    let skip = q.skip.unwrap_or(0);
    let limit = q.limit.unwrap_or(200).min(500);
    let body = tokio::task::spawn_blocking(move || {
        let Some(range) = r.range() else {
            return serde_json::json!({ "range": null, "commits": [], "graph": [] });
        };
        // One extra row tells us whether another page exists without counting
        // the whole range.
        let mut page = r.commits(&range.spec, skip, limit + 1);
        let more = page.len() > limit;
        page.truncate(limit);
        let spec: Vec<(String, Vec<String>)> = page
            .iter()
            .map(|c| (c.sha.clone(), c.parents.clone()))
            .collect();
        let graph = crate::graph::lay_out(&spec);
        serde_json::json!({
            "range": range,
            "commits": page,
            "graph": graph,
            "hasMore": more,
        })
    })
    .await
    .unwrap();
    Json(body).into_response()
}

/// Holding this stream open is what keeps diffuse alive; closing the tab is
/// how you quit it.
async fn events(State(app): State<App>, headers: HeaderMap, Query(q): Query<Auth>) -> Response {
    if !authorized(&app, &headers, q.t.as_deref()) {
        return unauthorized();
    }
    app.clients.fetch_add(1, Ordering::SeqCst);
    app.ever_connected.fetch_add(1, Ordering::SeqCst);
    let guard = ClientGuard(app.clients.clone());

    let stream =
        IntervalStream::new(tokio::time::interval(Duration::from_secs(5))).map(move |_| {
            let _hold = &guard;
            Ok::<_, Infallible>(Event::default().event("ping").data("1"))
        });
    Sse::new(stream).into_response()
}

struct ClientGuard(Arc<AtomicUsize>);
impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path).or_else(|| Assets::get("index.html")) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

pub struct Serving {
    pub url: String,
}

pub async fn serve(runner: Runner, dev: bool) -> std::io::Result<Serving> {
    let token = if dev {
        "dev".to_string()
    } else {
        random_token()
    };
    let app = App {
        runner: Arc::new(runner),
        token: Arc::new(token.clone()),
        clients: Arc::new(AtomicUsize::new(0)),
        ever_connected: Arc::new(AtomicUsize::new(0)),
    };

    let router = Router::new()
        .route("/api/session", get(session))
        .route("/api/files", get(files))
        .route("/api/file", get(file))
        .route("/api/all", get(all))
        .route("/api/commits", get(commits))
        .route("/api/events", get(events))
        .fallback(asset)
        .with_state(app.clone());

    let port = if dev { 5177 } else { 0 };
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    // Exit once the last tab goes away, so `diffuse` behaves like a command
    // rather than a daemon you have to remember to stop.
    if !dev {
        let clients = app.clients.clone();
        let ever = app.ever_connected.clone();
        tokio::spawn(async move {
            let mut idle = Duration::ZERO;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if std::env::var("DIFFUSE_TRACE").is_ok() {
                    eprintln!(
                        "idle-tick clients={} ever={} idle={:?}",
                        clients.load(Ordering::SeqCst),
                        ever.load(Ordering::SeqCst),
                        idle
                    );
                }
                if clients.load(Ordering::SeqCst) == 0 && ever.load(Ordering::SeqCst) > 0 {
                    idle += Duration::from_secs(1);
                    if idle >= IDLE_GRACE {
                        std::process::exit(0);
                    }
                } else {
                    idle = Duration::ZERO;
                }
            }
        });
    }

    Ok(Serving {
        url: format!("http://127.0.0.1:{}/?t={}", addr.port(), token),
    })
}

/// 128 bits from the operating system, hex encoded. This guards the only door
/// to the repository's contents, so it must come from a real CSPRNG.
fn random_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("no system randomness available");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
