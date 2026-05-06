//! Daemon socket lifecycle: bind, accept, drain, cleanup.
//!
//! Per-connection RPC dispatch lives in [`crate::rpc`]. This module owns
//! the listener, the signal-driven shutdown, and socket-file cleanup.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::net::UnixListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use crate::rpc::{self, AppState};

/// Maximum time to wait for in-flight handlers during shutdown before
/// abandoning them.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Bind the UDS at `path`, accept connections, drain on SIGTERM/SIGINT,
/// remove the socket file on exit.
///
/// Errors if `path` already exists (no silent overwrite of a live socket).
pub async fn serve(path: PathBuf) -> Result<()> {
    if path.exists() {
        bail!(
            "socket already exists at {} (is another daemon running?)",
            path.display()
        );
    }
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dir {}", parent.display()))?;
    }

    let listener =
        UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("setting permissions on {}", path.display()))?;

    let started_at = std::time::Instant::now();
    let state = Arc::new(AppState { started_at });
    info!(socket = %path.display(), version = ca_lib::version(), "ca-daemon listening");

    let shutdown = Arc::new(Notify::new());
    spawn_signal_listener(shutdown.clone());

    let mut conns: JoinSet<()> = JoinSet::new();

    loop {
        tokio::select! {
            biased;
            () = shutdown.notified() => {
                info!("shutdown signal received; stopping accept loop");
                break;
            }
            accept = listener.accept() => match accept {
                Ok((stream, _)) => {
                    debug!("connection accepted");
                    conns.spawn(rpc::handle_connection(stream, state.clone()));
                }
                Err(e) => warn!(error = %e, "accept error"),
            },
        }
    }

    // Drain in-flight handlers, bounded.
    let drain_outcome = tokio::time::timeout(DRAIN_TIMEOUT, async {
        while let Some(res) = conns.join_next().await {
            if let Err(e) = res {
                debug!(error = %e, "handler join error");
            }
        }
    })
    .await;

    if drain_outcome.is_err() {
        warn!(remaining = conns.len(), "drain timeout; aborting handlers");
        conns.abort_all();
        while conns.join_next().await.is_some() {}
    }

    match std::fs::remove_file(&path) {
        Ok(()) => info!(socket = %path.display(), "socket removed"),
        Err(e) => warn!(error = %e, socket = %path.display(), "could not remove socket file"),
    }

    info!(
        uptime_s = started_at.elapsed().as_secs(),
        "ca-daemon stopped"
    );
    Ok(())
}

fn spawn_signal_listener(shutdown: Arc<Notify>) {
    tokio::spawn(async move {
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "registering SIGTERM handler");
                return;
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "registering SIGINT handler");
                return;
            }
        };
        tokio::select! {
            _ = sigterm.recv() => info!("SIGTERM received"),
            _ = sigint.recv() => info!("SIGINT received"),
        }
        shutdown.notify_waiters();
    });
}
