//! `guide-history-service` — a small, always-on, GUI-free companion to
//! DVRDesk Native. Channels DVR's own guide API is forward-only (confirmed
//! directly against a real server: the XMLTV feed, the per-series airings
//! lookup, and every other guide endpoint never return anything before
//! "now"), so seeing "what was on yesterday" in the desktop app's guide
//! grid requires capturing slots *before* they age out — continuously,
//! independent of whether any desktop client happens to be running. That's
//! this service's entire job: poll the DVR's XMLTV feed on an interval,
//! keep a rolling window of what it's seen in one JSON file, and serve it
//! back out over a tiny HTTP API so any number of desktop clients on the
//! network can read it.
//!
//! Configuration is environment variables — the path of least resistance
//! under systemd/Docker/a NAS's task scheduler — or the equivalent
//! `--flag value` command-line options, which win when both are given.
//! Windows Task Scheduler (how DVRDesk installs this on Windows) can't set
//! a task's environment, only its arguments.

mod guide_program;
mod store;

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

#[derive(Clone)]
struct AppState {
    store: Arc<store::Store>,
}

#[derive(Deserialize)]
struct HistoryQuery {
    from: i64,
    to: i64,
}

async fn history_handler(
    State(state): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> Json<Vec<guide_program::GuideProgram>> {
    Json(state.store.query(q.from, q.to))
}

async fn health_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "programs_cached": state.store.len() }))
}

/// `--name value` / `--name=value` pairs from the command line.
fn parse_args() -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let Some(flag) = arg.strip_prefix("--") else {
            eprintln!("guide-history-service: ignoring unexpected argument {arg:?}");
            continue;
        };
        match flag.split_once('=') {
            Some((name, value)) => {
                out.insert(name.to_string(), value.to_string());
            }
            None => match args.next() {
                Some(value) => {
                    out.insert(flag.to_string(), value);
                }
                None => eprintln!("guide-history-service: --{flag} needs a value"),
            },
        }
    }
    out
}

struct Config {
    args: std::collections::HashMap<String, String>,
}

impl Config {
    /// The command-line flag if given, else the first set environment variable.
    fn get(&self, flag: &str, envs: &[&str]) -> Option<String> {
        self.args
            .get(flag)
            .cloned()
            .or_else(|| envs.iter().find_map(|name| std::env::var(name).ok()))
    }

    fn string(&self, flag: &str, env: &str, default: &str) -> String {
        self.get(flag, &[env]).unwrap_or_else(|| default.to_string())
    }

    fn parsed<T: std::str::FromStr>(&self, flag: &str, env: &str, default: T) -> T {
        self.get(flag, &[env])
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }
}

#[tokio::main]
async fn main() {
    // The Channels DVR server to poll — not this service's own address.
    // `GHS_SERVER_URL` is the original name, still accepted; "server" read
    // as ambiguous to users setting this up by hand.
    let config = Config { args: parse_args() };
    let server_url = config
        .get("channels-dvr-url", &["GHS_CHANNELS_DVR_URL", "GHS_SERVER_URL"])
        .unwrap_or_else(|| {
            eprintln!("guide-history-service: set your Channels DVR server's address with --channels-dvr-url or GHS_CHANNELS_DVR_URL, e.g. http://192.168.1.10:8089");
            std::process::exit(1);
        });
    // How often to re-poll the DVR's guide feed.
    let poll_secs: u64 = config.parsed("poll-secs", "GHS_POLL_SECS", 900);
    // How long a captured slot is kept after it airs.
    let retention_secs: i64 = config.parsed("retention-secs", "GHS_RETENTION_SECS", 172_800);
    // How far forward each poll asks the DVR for — only needs to be wide
    // enough that no slot goes from "not yet in the feed" to "already aired"
    // between two consecutive polls.
    let fetch_window_secs: u32 = config.parsed("fetch-window-secs", "GHS_FETCH_WINDOW_SECS", 7_200);
    let listen_addr = config.string("listen", "GHS_LISTEN_ADDR", "0.0.0.0:8790");
    let data_path = config.string("data", "GHS_DATA_PATH", "guide-history.json");

    let store = Arc::new(store::Store::load(data_path.clone().into()));
    eprintln!(
        "guide-history-service: loaded {} cached programs from {data_path}",
        store.len()
    );

    {
        let store = store.clone();
        let server_url = server_url.clone();
        tokio::spawn(async move {
            loop {
                match guide_program::fetch_guide(&server_url, fetch_window_secs).await {
                    Ok(programs) => {
                        let fetched = programs.len();
                        store.merge(programs);
                        let cutoff = chrono::Utc::now().timestamp() - retention_secs;
                        store.prune(cutoff);
                        match store.save() {
                            Ok(()) => eprintln!(
                                "guide-history-service: captured {fetched} slots this poll, {} total in store",
                                store.len()
                            ),
                            Err(e) => eprintln!("guide-history-service: failed to save store: {e}"),
                        }
                    }
                    Err(e) => eprintln!("guide-history-service: guide fetch failed: {e}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(poll_secs)).await;
            }
        });
    }

    let state = AppState { store };
    let app = Router::new()
        .route("/history", get(history_handler))
        .route("/health", get(health_handler))
        .with_state(state);

    eprintln!("guide-history-service: listening on {listen_addr}");
    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("guide-history-service: failed to bind {listen_addr}: {e}");
            std::process::exit(1);
        });
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("guide-history-service: server error: {e}");
        std::process::exit(1);
    }
}
