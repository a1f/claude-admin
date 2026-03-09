use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ca_lib::db::Database;
use ca_lib::models::{Session, SessionState};
use ca_lib::notify::NotificationConfig;
use tokio::sync::broadcast;
use tokio::time;

use crate::notifier::Notifier;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub async fn run_polling_loop(
    db: Arc<Mutex<Database>>,
    mut shutdown_rx: broadcast::Receiver<()>,
    update_tx: broadcast::Sender<Vec<Session>>,
) {
    let config = {
        let db = db.lock().expect("database mutex poisoned");
        NotificationConfig::from_settings(&db)
    };
    let mut notifier = Notifier::new(config);
    let mut previous_states: HashMap<String, SessionState> = HashMap::new();

    let mut interval = time::interval(POLL_INTERVAL);
    // First tick fires immediately -- skip it to let daemon fully initialize
    interval.tick().await;

    tracing::info!(
        interval_secs = POLL_INTERVAL.as_secs(),
        "Polling loop started"
    );

    loop {
        tokio::select! {
            _ = interval.tick() => {
                poll_once(&db, &update_tx, &mut notifier, &mut previous_states).await;
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
    notifier: &mut Notifier,
    previous_states: &mut HashMap<String, SessionState>,
) {
    let db_clone = Arc::clone(db);
    let result = tokio::task::spawn_blocking(move || {
        let db = db_clone.lock().expect("database mutex poisoned");
        ca_lib::discovery::sync_sessions(&db)
    })
    .await;

    match result {
        Ok(Ok(sync)) => {
            let has_changes =
                !sync.discovered.is_empty() || !sync.updated.is_empty() || !sync.removed.is_empty();

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

            check_notifications(db, notifier, previous_states).await;
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Discovery poll failed");
        }
        Err(e) => {
            tracing::error!(error = %e, "Discovery poll task panicked");
        }
    }
}

/// Compare current session states against previous snapshot, fire
/// notifications for transitions, then update the snapshot.
async fn check_notifications(
    db: &Arc<Mutex<Database>>,
    notifier: &mut Notifier,
    previous_states: &mut HashMap<String, SessionState>,
) {
    let db_clone = Arc::clone(db);
    let sessions = tokio::task::spawn_blocking(move || {
        let db = db_clone.lock().expect("database mutex poisoned");
        db.list_sessions()
    })
    .await;

    let sessions = match sessions {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Failed to fetch sessions for notifications");
            return;
        }
        Err(e) => {
            tracing::error!(error = %e, "Notification session query task panicked");
            return;
        }
    };

    for session in &sessions {
        if let Some(prev_state) = previous_states.get(&session.id) {
            if *prev_state != session.state {
                notifier.check_and_notify(&session.id, prev_state, &session.state);
            }
        }
    }

    // Rebuild previous_states from current snapshot
    let active_ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    notifier.cleanup_stale(&active_ids);

    previous_states.clear();
    for session in sessions {
        previous_states.insert(session.id, session.state);
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
