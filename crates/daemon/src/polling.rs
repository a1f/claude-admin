use ca_lib::db::Database;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub async fn run_polling_loop(
    db: Arc<Mutex<Database>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let mut interval = time::interval(POLL_INTERVAL);
    // First tick fires immediately — skip it to let daemon fully initialize
    interval.tick().await;

    tracing::info!(interval_secs = POLL_INTERVAL.as_secs(), "Polling loop started");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                poll_once(&db).await;
            }
            _ = shutdown_rx.recv() => {
                tracing::info!("Polling loop received shutdown signal");
                break;
            }
        }
    }
}

async fn poll_once(db: &Arc<Mutex<Database>>) {
    let db = Arc::clone(db);
    let result = tokio::task::spawn_blocking(move || {
        let db = db.lock().expect("database mutex poisoned");
        ca_lib::discovery::sync_sessions(&db)
    })
    .await;

    match result {
        Ok(Ok(sync)) => {
            if !sync.discovered.is_empty()
                || !sync.updated.is_empty()
                || !sync.removed.is_empty()
            {
                tracing::info!(
                    discovered = sync.discovered.len(),
                    updated = sync.updated.len(),
                    removed = sync.removed.len(),
                    "Discovery poll complete"
                );
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
