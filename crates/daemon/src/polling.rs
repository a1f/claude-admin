use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ca_lib::db::Database;
use ca_lib::models::{Session, SessionState};
use ca_lib::notify::{NotificationConfig, send_review_notification};
use ca_lib::review::ReviewStatus;
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

    // Auto-create reviews for sessions that just transitioned to Done
    let newly_done: Vec<Session> = sessions
        .iter()
        .filter(|s| {
            s.state == SessionState::Done
                && s.project_id.is_some()
                && previous_states
                    .get(&s.id)
                    .is_some_and(|prev| *prev != SessionState::Done)
        })
        .cloned()
        .collect();

    if !newly_done.is_empty() {
        let db_clone = Arc::clone(db);
        let _ = tokio::task::spawn_blocking(move || {
            let db = db_clone.lock().expect("database mutex poisoned");
            for session in &newly_done {
                check_review_lifecycle(&db, session);
            }
        })
        .await;
    }

    // Rebuild previous_states from current snapshot
    let active_ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    notifier.cleanup_stale(&active_ids);

    previous_states.clear();
    for session in sessions {
        previous_states.insert(session.id, session.state);
    }
}

/// Auto-create a review when a session completes, if one doesn't already exist.
fn check_review_lifecycle(db: &Database, session: &Session) {
    let Some(project_id) = session.project_id else {
        return;
    };

    if let Ok(reviews) = db.list_reviews_by_session(&session.id) {
        let has_active = reviews
            .iter()
            .any(|r| r.status == ReviewStatus::Pending || r.status == ReviewStatus::InProgress);
        if has_active {
            return;
        }
    }

    // Branch/commit info left empty -- the TUI populates these when the user opens the review
    match db.create_review(Some(&session.id), Some(project_id), "", "", "") {
        Ok(_) => {
            tracing::info!(session_id = %session.id, "Auto-created review for completed session");
            if let Err(e) = send_review_notification(&session.id) {
                tracing::warn!(error = %e, "Failed to send review notification");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, session_id = %session.id, "Failed to auto-create review");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(id: &str, state: SessionState, project_id: Option<i64>) -> Session {
        Session {
            id: id.to_string(),
            pane_id: String::new(),
            session_name: String::new(),
            window_index: 0,
            pane_index: 0,
            working_dir: String::new(),
            state,
            detection_method: String::new(),
            last_activity: 0,
            created_at: 0,
            updated_at: 0,
            project_id,
            plan_step_id: None,
        }
    }

    fn create_test_db() -> (Database, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        (db, dir)
    }

    fn create_test_project(db: &Database) -> i64 {
        let ws = db
            .create_workspace("/tmp/test-project", Some("test"))
            .unwrap();
        db.create_project(ws.id, "Test Project", None).unwrap().id
    }

    #[test]
    fn test_check_review_lifecycle_creates_review() {
        let (db, _dir) = create_test_db();
        let project_id = create_test_project(&db);
        let session = make_session("sess-1", SessionState::Done, Some(project_id));

        check_review_lifecycle(&db, &session);

        let reviews = db.list_reviews_by_session("sess-1").unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].status, ReviewStatus::Pending);
        assert_eq!(reviews[0].session_id.as_deref(), Some("sess-1"));
        assert_eq!(reviews[0].project_id, Some(project_id));
    }

    #[test]
    fn test_check_review_lifecycle_no_project() {
        let (db, _dir) = create_test_db();
        let session = make_session("sess-2", SessionState::Done, None);

        check_review_lifecycle(&db, &session);

        let reviews = db.list_reviews_by_session("sess-2").unwrap();
        assert!(reviews.is_empty());
    }

    #[test]
    fn test_check_review_lifecycle_existing_active() {
        let (db, _dir) = create_test_db();
        let project_id = create_test_project(&db);

        // Pre-create a pending review for this session
        db.create_review(Some("sess-3"), Some(project_id), "main", "aaa", "bbb")
            .unwrap();

        let session = make_session("sess-3", SessionState::Done, Some(project_id));
        check_review_lifecycle(&db, &session);

        let reviews = db.list_reviews_by_session("sess-3").unwrap();
        assert_eq!(reviews.len(), 1, "should not create a duplicate review");
    }

    #[test]
    fn test_check_review_lifecycle_creates_after_completed_review() {
        let (db, _dir) = create_test_db();
        let project_id = create_test_project(&db);

        // Create a review and mark it as approved (no longer active)
        let review = db
            .create_review(Some("sess-4"), Some(project_id), "main", "aaa", "bbb")
            .unwrap();
        db.update_review_status(review.id, ReviewStatus::Approved)
            .unwrap();

        let session = make_session("sess-4", SessionState::Done, Some(project_id));
        check_review_lifecycle(&db, &session);

        let reviews = db.list_reviews_by_session("sess-4").unwrap();
        assert_eq!(
            reviews.len(),
            2,
            "should create a new review after prior was approved"
        );
        assert!(reviews.iter().any(|r| r.status == ReviewStatus::Pending));
    }
}
