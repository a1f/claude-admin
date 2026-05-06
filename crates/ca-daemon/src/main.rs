//! ca-daemon — local orchestrator daemon for claude_admin v1.
//!
//! Entry point. Sets up tracing, resolves the socket path, hands off to
//! the socket module which owns the lifecycle.

use std::path::PathBuf;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

mod db;
mod socket;

/// Resolve the socket path from the `CA_SOCKET_PATH` env var (test override)
/// or fall back to `$HOME/.work/ca.sock`.
fn resolve_socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CA_SOCKET_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").expect("HOME env var must be set");
    PathBuf::from(home).join(".work").join("ca.sock")
}

/// Resolve the SQLite path from the `CA_DB_PATH` env var (test override)
/// or fall back to `$HOME/.work/ca.db`.
fn resolve_db_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CA_DB_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").expect("HOME env var must be set");
    PathBuf::from(home).join(".work").join("ca.db")
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    let socket_path = resolve_socket_path();
    let db_path = resolve_db_path();

    let pool = match db::open(&db_path).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = ?e, "db open failed");
            eprintln!("ca-daemon: db: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let architectors = db::count_architectors(&pool).await.unwrap_or(-1);
    let tasks = db::count_tasks(&pool).await.unwrap_or(-1);
    tracing::info!(
        architectors,
        tasks,
        db = %db_path.display(),
        "DB ready"
    );

    // Hold the pool for the lifetime of the process. M2+ wires it through
    // socket::serve so the RPC handlers can read/write.
    let _pool = pool;

    match socket::serve(socket_path).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = ?e, "daemon exited with error");
            // Also print to stderr so the error is visible without RUST_LOG.
            eprintln!("ca-daemon: {e:#}");
            ExitCode::FAILURE
        }
    }
}
