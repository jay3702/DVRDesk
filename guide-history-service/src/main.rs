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
//! Configuration is environment variables only (no CLI flags, no config
//! file format of its own) — this is meant to run unattended under
//! systemd/Docker/a NAS's task scheduler, all of which make env vars the
//! path of least resistance.

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

fn env_var(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn env_var_parsed<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::main]
async fn main() {
    let server_url = std::env::var("GHS_SERVER_URL").unwrap_or_else(|_| {
        eprintln!("guide-history-service: GHS_SERVER_URL must be set to your Channels DVR server, e.g. http://192.168.1.10:8089");
        std::process::exit(1);
    });
    // How often to re-poll the DVR's guide feed.
    let poll_secs: u64 = env_var_parsed("GHS_POLL_SECS", 900);
    // How long a captured slot is kept after it airs.
    let retention_secs: i64 = env_var_parsed("GHS_RETENTION_SECS", 172_800);
    // How far forward each poll asks the DVR for — only needs to be wide
    // enough that no slot goes from "not yet in the feed" to "already aired"
    // between two consecutive polls.
    let fetch_window_secs: u32 = env_var_parsed("GHS_FETCH_WINDOW_SECS", 7_200);
    let listen_addr = env_var("GHS_LISTEN_ADDR", "0.0.0.0:8790");
    let data_path = env_var("GHS_DATA_PATH", "guide-history.json");

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
