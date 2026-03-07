use crate::db::{Database, DbError};
use crate::events::EventType;
use crate::models::{Session, SessionState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookEvent {
    pub hook_type: String,
    pub session_id: Option<String>,
    pub working_dir: String,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Error, Debug)]
pub enum HookError {
    #[error("database error: {0}")]
    Db(#[from] DbError),
}

pub fn infer_state_from_hook(hook_type: &str) -> Option<SessionState> {
    match hook_type {
        "PreToolUse" | "PostToolUse" | "UserPromptSubmit" => Some(SessionState::Working),
        "Stop" | "SessionEnd" => Some(SessionState::Done),
        "Notification" => Some(SessionState::NeedsInput),
        "SessionStart" => Some(SessionState::Idle),
        _ => None,
    }
}

pub fn find_session_for_hook(
    db: &Database,
    working_dir: &str,
) -> Result<Option<Session>, DbError> {
    let sessions = db.list_sessions()?;
    Ok(sessions.into_iter().find(|s| s.working_dir == working_dir))
}

pub fn apply_hook_event(
    db: &Database,
    event: &HookEvent,
) -> Result<Option<String>, HookError> {
    let session = match find_session_for_hook(db, &event.working_dir)? {
        Some(s) => s,
        None => return Ok(None),
    };

    if let Some(new_state) = infer_state_from_hook(&event.hook_type) {
        db.update_session_state(&session.id, new_state, event.timestamp)?;
    }

    db.log_event(
        &session.id,
        &EventType::HookReceived {
            hook_type: event.hook_type.clone(),
        },
        event.payload.as_ref(),
    )?;

    Ok(Some(session.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db() -> (Database, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.db")).unwrap();
        (db, dir)
    }

    fn make_session(id: &str, pane_id: &str, working_dir: &str) -> Session {
        Session {
            id: id.to_string(),
            pane_id: pane_id.to_string(),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: working_dir.to_string(),
            state: SessionState::Idle,
            detection_method: "process_name".to_string(),
            last_activity: 1706500000,
            created_at: 1706400000,
            updated_at: 1706500000,
            project_id: None,
            plan_step_id: None,
        }
    }

    // -- Group 1: infer_state_from_hook --

    #[test]
    fn test_infer_pre_tool_use() {
        assert_eq!(infer_state_from_hook("PreToolUse"), Some(SessionState::Working));
    }

    #[test]
    fn test_infer_post_tool_use() {
        assert_eq!(infer_state_from_hook("PostToolUse"), Some(SessionState::Working));
    }

    #[test]
    fn test_infer_stop() {
        assert_eq!(infer_state_from_hook("Stop"), Some(SessionState::Done));
    }

    #[test]
    fn test_infer_notification() {
        assert_eq!(
            infer_state_from_hook("Notification"),
            Some(SessionState::NeedsInput)
        );
    }

    #[test]
    fn test_infer_user_prompt_submit() {
        assert_eq!(
            infer_state_from_hook("UserPromptSubmit"),
            Some(SessionState::Working)
        );
    }

    #[test]
    fn test_infer_session_start() {
        assert_eq!(infer_state_from_hook("SessionStart"), Some(SessionState::Idle));
    }

    #[test]
    fn test_infer_session_end() {
        assert_eq!(infer_state_from_hook("SessionEnd"), Some(SessionState::Done));
    }

    #[test]
    fn test_infer_unknown_returns_none() {
        assert_eq!(infer_state_from_hook("UnrecognizedHookType"), None);
        assert_eq!(infer_state_from_hook("posttooluse"), None);
    }

    // -- Group 2: HookEvent serde --

    #[test]
    fn test_hook_event_serde_with_payload() {
        let event = HookEvent {
            hook_type: "PostToolUse".to_string(),
            session_id: Some("sess-1".to_string()),
            working_dir: "/home/user/project".to_string(),
            timestamp: 1706500000,
            payload: Some(serde_json::json!({"tool": "Read", "path": "/foo"})),
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn test_hook_event_serde_without_payload() {
        let event = HookEvent {
            hook_type: "Stop".to_string(),
            session_id: None,
            working_dir: "/home/user/project".to_string(),
            timestamp: 1706500000,
            payload: None,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("payload"));

        let parsed: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    // -- Group 3: find_session_for_hook --

    #[test]
    fn test_find_session_match() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", "/home/user/myproject");
        db.create_session(&session).unwrap();

        let found = find_session_for_hook(&db, "/home/user/myproject").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "sess-1");
    }

    #[test]
    fn test_find_session_no_match() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", "/home/user/myproject");
        db.create_session(&session).unwrap();

        let found = find_session_for_hook(&db, "/home/user/other").unwrap();
        assert!(found.is_none());
    }

    // -- Group 4: apply_hook_event --

    #[test]
    fn test_apply_hook_event_updates_state() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", "/home/user/project");
        db.create_session(&session).unwrap();

        let event = HookEvent {
            hook_type: "PostToolUse".to_string(),
            session_id: None,
            working_dir: "/home/user/project".to_string(),
            timestamp: 1706600000,
            payload: None,
        };

        let result = apply_hook_event(&db, &event).unwrap();
        assert_eq!(result, Some("sess-1".to_string()));

        let updated = db.get_session("sess-1").unwrap().unwrap();
        assert_eq!(updated.state, SessionState::Working);
        assert_eq!(updated.last_activity, 1706600000);
        assert_eq!(updated.updated_at, 1706600000);
    }

    #[test]
    fn test_apply_hook_event_logs_event() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", "/home/user/project");
        db.create_session(&session).unwrap();

        let event = HookEvent {
            hook_type: "Stop".to_string(),
            session_id: None,
            working_dir: "/home/user/project".to_string(),
            timestamp: 1706600000,
            payload: None,
        };

        apply_hook_event(&db, &event).unwrap();

        let events = db.get_events("sess-1", 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].event_type,
            EventType::HookReceived {
                hook_type: "Stop".to_string()
            }
        );
    }

    #[test]
    fn test_apply_hook_event_no_session() {
        let (db, _dir) = make_db();

        let event = HookEvent {
            hook_type: "PostToolUse".to_string(),
            session_id: None,
            working_dir: "/nonexistent".to_string(),
            timestamp: 1706600000,
            payload: None,
        };

        let result = apply_hook_event(&db, &event).unwrap();
        assert_eq!(result, None);

        let events = db.get_recent_events(10).unwrap();
        assert!(events.is_empty());
    }
}
