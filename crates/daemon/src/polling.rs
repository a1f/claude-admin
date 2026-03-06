use ca_lib::db::Database;
use ca_lib::models::Session;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub async fn run_polling_loop(
    db: Arc<Mutex<Database>>,
    mut shutdown_rx: broadcast::Receiver<()>,
    update_tx: broadcast::Sender<Vec<Session>>,
) {
    let mut interval = time::interval(POLL_INTERVAL);
    // First tick fires immediately -- skip it to let daemon fully initialize
    interval.tick().await;

    tracing::info!(interval_secs = POLL_INTERVAL.as_secs(), "Polling loop started");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                poll_once(&db, &update_tx).await;
            }
            _ = shutdown_rx.recv() => {
                tracing::info!("Polling loop received shutdown signal");
                break;
            }
        }
    }
}

async fn poll_once(
    db: &Arc<Mutex<Database>>,
    update_tx: &broadcast::Sender<Vec<Session>>,
) {
    let db_clone = Arc::clone(db);
    let result = tokio::task::spawn_blocking(move || {
        let db = db_clone.lock().expect("database mutex poisoned");
        ca_lib::discovery::sync_sessions(&db)
    })
    .await;

    match result {
        Ok(Ok(sync)) => {
            let has_changes = !sync.discovered.is_empty()
                || !sync.updated.is_empty()
                || !sync.removed.is_empty();

            if has_changes {
                tracing::info!(
                    discovered = sync.discovered.len(),
                    updated = sync.updated.len(),
                    removed = sync.removed.len(),
                    "Discovery poll complete"
                );
                broadcast_sessions(db, update_tx).await;
            } else {
                tracing::trace!("Discovery poll: no changes");
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Discovery poll failed");
        }
        Err(e) => {
            tracing::error!(error = %e, "Discovery poll task panicked");
        }
    }
}

/// Fetch current session list from DB and broadcast to all subscribers.
pub async fn broadcast_sessions(
    db: &Arc<Mutex<Database>>,
    update_tx: &broadcast::Sender<Vec<Session>>,
) {
    // No receivers subscribed -- skip the DB query
    if update_tx.receiver_count() == 0 {
        return;
    }

    let db_clone = Arc::clone(db);
    let sessions = tokio::task::spawn_blocking(move || {
        let db = db_clone.lock().expect("database mutex poisoned");
        db.list_sessions()
    })
    .await;

    match sessions {
        Ok(Ok(sessions)) => {
            let _ = update_tx.send(sessions);
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Failed to fetch sessions for broadcast");
        }
        Err(e) => {
            tracing::error!(error = %e, "Broadcast session query task panicked");
        }
    }
}
