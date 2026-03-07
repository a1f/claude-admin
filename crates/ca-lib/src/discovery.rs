use crate::db::{Database, DbError};
use crate::events::EventType;
use crate::models::{Session, SessionState};
use crate::state::detect_state;
use crate::tmux::{
    capture_pane_content, get_pane_process, is_tmux_running, list_all_panes, ClaudeLocation,
    DetectionMethod, TmuxError, TmuxPane,
};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

const CONTENT_CAPTURE_LINES: u32 = 20;

#[derive(Error, Debug)]
pub enum DiscoveryError {
    #[error("tmux error: {0}")]
    Tmux(#[from] TmuxError),
    #[error("database error: {0}")]
    Db(#[from] DbError),
}

/// Result of a sync_sessions call.
#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    pub discovered: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
}

/// Check if a process name indicates a Claude Code session.
///
/// Matches: "claude" (substring, case-insensitive), "node", "deno",
/// and version-like strings (e.g. "1.0.12").
pub fn is_claude_process(process_name: &str) -> bool {
    let trimmed = process_name.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_lowercase();

    // Direct claude match (covers "claude", "claude-code", etc.)
    if lower.contains("claude") {
        return true;
    }

    // Claude Code often appears as node or deno
    if lower == "node" || lower == "deno" {
        return true;
    }

    // Version number pattern (e.g. "1.0.12") — Claude binary sometimes shows this way
    trimmed.starts_with(|c: char| c.is_ascii_digit()) && trimmed.contains('.')
}

/// Scan all tmux panes and return locations where Claude appears to be running.
pub fn discover_claude_panes() -> Result<Vec<ClaudeLocation>, DiscoveryError> {
    if !is_tmux_running() {
        return Ok(Vec::new());
    }

    let panes = list_all_panes()?;
    let now = now_unix();
    let mut locations = Vec::new();

    for pane in &panes {
        if let Some(location) = check_pane(pane, now) {
            locations.push(location);
        }
    }

    tracing::debug!(count = locations.len(), total_panes = panes.len(), "Discovery scan complete");
    Ok(locations)
}

/// Check a single pane for Claude. Tries process name first, then content.
fn check_pane(pane: &TmuxPane, now: i64) -> Option<ClaudeLocation> {
    // Try process name detection first
    if let Ok(process) = get_pane_process(&pane.pane_id) {
        if is_claude_process(&process) {
            return Some(ClaudeLocation {
                pane: pane.clone(),
                detection_method: DetectionMethod::ProcessName,
                detected_at: now,
            });
        }
    }

    // Fall back to content-based detection
    if let Ok(content) = capture_pane_content(&pane.pane_id, CONTENT_CAPTURE_LINES) {
        let state = detect_state(&content);
        if state != SessionState::Idle {
            return Some(ClaudeLocation {
                pane: pane.clone(),
                detection_method: DetectionMethod::PaneContent,
                detected_at: now,
            });
        }
    }

    None
}

/// Synchronize database sessions with live tmux state.
///
/// - Creates new sessions for newly discovered Claude panes
/// - Updates state for existing sessions
/// - Removes sessions whose panes no longer exist
pub fn sync_sessions(db: &Database) -> Result<SyncResult, DiscoveryError> {
    let locations = discover_claude_panes()?;
    let now = now_unix();
    let mut result = SyncResult::default();

    let active_pane_ids: HashSet<String> =
        locations.iter().map(|l| l.pane.pane_id.clone()).collect();

    for location in &locations {
        match db.get_session_by_pane(&location.pane.pane_id)? {
            Some(existing) => {
                // Update state if changed
                let content =
                    capture_pane_content(&location.pane.pane_id, CONTENT_CAPTURE_LINES)
                        .unwrap_or_default();
                let new_state = detect_state(&content);

                if new_state != existing.state {
                    let old_state = existing.state;
                    db.update_session_state(&existing.id, new_state, now)?;
                    db.log_event(
                        &existing.id,
                        &EventType::StateChanged {
                            from: old_state,
                            to: new_state,
                        },
                        None,
                    )?;
                    result.updated.push(existing.id.clone());
                    tracing::info!(
                        session_id = %existing.id,
                        pane_id = %existing.pane_id,
                        from = %old_state,
                        to = %new_state,
                        "Session state changed"
                    );
                }
            }
            None => {
                // New session
                let content =
                    capture_pane_content(&location.pane.pane_id, CONTENT_CAPTURE_LINES)
                        .unwrap_or_default();
                let state = detect_state(&content);

                let session = Session {
                    id: Uuid::new_v4().to_string(),
                    pane_id: location.pane.pane_id.clone(),
                    session_name: location.pane.session_name.clone(),
                    window_index: location.pane.window_index,
                    pane_index: location.pane.pane_index,
                    working_dir: location.pane.working_dir.clone(),
                    state,
                    detection_method: location.detection_method.to_string(),
                    last_activity: now,
                    created_at: now,
                    updated_at: now,
                    project_id: None,
                    plan_step_id: None,
                };

                db.create_session(&session)?;
                db.log_event(&session.id, &EventType::SessionDiscovered, None)?;
                tracing::info!(
                    session_id = %session.id,
                    pane_id = %session.pane_id,
                    method = %session.detection_method,
                    "New Claude session discovered"
                );
                result.discovered.push(session.id);
            }
        }
    }

    // Remove stale sessions
    result.removed = cleanup_stale_sessions(db, &active_pane_ids)?;

    Ok(result)
}

/// Remove sessions from DB whose panes are no longer active.
///
/// Deletes associated events first to satisfy FK constraints,
/// then removes the session row.
fn cleanup_stale_sessions(
    db: &Database,
    active_pane_ids: &HashSet<String>,
) -> Result<Vec<String>, DiscoveryError> {
    let existing = db.list_sessions()?;
    let mut removed = Vec::new();

    for session in existing {
        if !active_pane_ids.contains(&session.pane_id) {
            db.delete_events_for_session(&session.id)?;
            db.delete_session(&session.id)?;
            tracing::info!(
                session_id = %session.id,
                pane_id = %session.pane_id,
                "Stale session removed"
            );
            removed.push(session.id);
        }
    }

    Ok(removed)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_db() -> (Database, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        (db, dir)
    }

    fn create_test_session(id: &str, pane_id: &str) -> Session {
        Session {
            id: id.to_string(),
            pane_id: pane_id.to_string(),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: "/home/user".to_string(),
            state: SessionState::Idle,
            detection_method: "process_name".to_string(),
            last_activity: 1706500000,
            created_at: 1706400000,
            updated_at: 1706500000,
            project_id: None,
            plan_step_id: None,
        }
    }

    // --- is_claude_process tests ---

    #[test]
    fn test_is_claude_process_direct_match() {
        assert!(is_claude_process("claude"));
    }

    #[test]
    fn test_is_claude_process_claude_code() {
        assert!(is_claude_process("claude-code"));
    }

    #[test]
    fn test_is_claude_process_case_insensitive() {
        assert!(is_claude_process("Claude"));
        assert!(is_claude_process("CLAUDE"));
        assert!(is_claude_process("Claude-Code"));
    }

    #[test]
    fn test_is_claude_process_node() {
        assert!(is_claude_process("node"));
    }

    #[test]
    fn test_is_claude_process_deno() {
        assert!(is_claude_process("deno"));
    }

    #[test]
    fn test_is_claude_process_version_pattern() {
        assert!(is_claude_process("1.0.12"));
        assert!(is_claude_process("2.3.0"));
        assert!(is_claude_process("0.1.0"));
    }

    #[test]
    fn test_is_claude_process_rejects_common_shells() {
        assert!(!is_claude_process("bash"));
        assert!(!is_claude_process("zsh"));
        assert!(!is_claude_process("fish"));
        assert!(!is_claude_process("sh"));
    }

    #[test]
    fn test_is_claude_process_rejects_editors() {
        assert!(!is_claude_process("vim"));
        assert!(!is_claude_process("nvim"));
        assert!(!is_claude_process("emacs"));
    }

    #[test]
    fn test_is_claude_process_rejects_empty() {
        assert!(!is_claude_process(""));
        assert!(!is_claude_process("   "));
    }

    #[test]
    fn test_is_claude_process_handles_whitespace() {
        assert!(is_claude_process("  claude  "));
        assert!(is_claude_process("  node  "));
    }

    #[test]
    fn test_is_claude_process_rejects_partial_node() {
        // "nodejs" is not "node" — should not match (no claude substring)
        assert!(!is_claude_process("nodejs"));
    }

    // --- cleanup_stale_sessions tests ---

    #[test]
    fn test_cleanup_removes_stale_sessions() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        let active: HashSet<String> = HashSet::new(); // no active panes
        let removed = cleanup_stale_sessions(&db, &active).unwrap();

        assert_eq!(removed, vec!["sess-1"]);
        assert!(db.get_session("sess-1").unwrap().is_none());
    }

    #[test]
    fn test_cleanup_preserves_active_sessions() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();

        let active: HashSet<String> = ["%0".to_string()].into();
        let removed = cleanup_stale_sessions(&db, &active).unwrap();

        assert!(removed.is_empty());
        assert!(db.get_session("sess-1").unwrap().is_some());
    }

    #[test]
    fn test_cleanup_mixed_active_and_stale() {
        let (db, _dir) = create_test_db();
        db.create_session(&create_test_session("sess-1", "%0")).unwrap();
        db.create_session(&create_test_session("sess-2", "%1")).unwrap();
        db.create_session(&create_test_session("sess-3", "%2")).unwrap();

        let active: HashSet<String> = ["%1".to_string()].into();
        let removed = cleanup_stale_sessions(&db, &active).unwrap();

        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&"sess-1".to_string()));
        assert!(removed.contains(&"sess-3".to_string()));
        assert!(db.get_session("sess-2").unwrap().is_some());
    }

    #[test]
    fn test_cleanup_empty_db() {
        let (db, _dir) = create_test_db();

        let active: HashSet<String> = HashSet::new();
        let removed = cleanup_stale_sessions(&db, &active).unwrap();

        assert!(removed.is_empty());
    }

    #[test]
    fn test_cleanup_deletes_associated_events() {
        let (db, _dir) = create_test_db();
        let session = create_test_session("sess-1", "%0");
        db.create_session(&session).unwrap();
        db.log_event("sess-1", &EventType::SessionDiscovered, None)
            .unwrap();

        let active: HashSet<String> = HashSet::new();
        let removed = cleanup_stale_sessions(&db, &active).unwrap();

        assert_eq!(removed, vec!["sess-1"]);
        assert!(db.get_session("sess-1").unwrap().is_none());
        // Events for removed sessions are cleaned up (FK constraint)
        let events = db.get_recent_events(10).unwrap();
        assert!(events.is_empty());
    }

    // --- SyncResult tests ---

    #[test]
    fn test_sync_result_default() {
        let result = SyncResult::default();
        assert!(result.discovered.is_empty());
        assert!(result.updated.is_empty());
        assert!(result.removed.is_empty());
    }

    // --- DiscoveryError tests ---

    #[test]
    fn test_discovery_error_from_tmux() {
        let tmux_err = TmuxError::NotRunning;
        let disc_err: DiscoveryError = tmux_err.into();
        assert!(matches!(disc_err, DiscoveryError::Tmux(_)));
        assert!(disc_err.to_string().contains("tmux"));
    }

    #[test]
    fn test_discovery_error_display() {
        let err = DiscoveryError::Tmux(TmuxError::NotRunning);
        assert_eq!(err.to_string(), "tmux error: tmux not running");
    }
}
