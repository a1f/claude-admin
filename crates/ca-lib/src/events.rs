use crate::models::SessionState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventType {
    SessionDiscovered,
    SessionRemoved,
    StateChanged {
        from: SessionState,
        to: SessionState,
    },
    HookReceived {
        hook_type: String,
    },
}

impl EventType {
    pub fn type_name(&self) -> &'static str {
        match self {
            EventType::SessionDiscovered => "session_discovered",
            EventType::SessionRemoved => "session_removed",
            EventType::StateChanged { .. } => "state_changed",
            EventType::HookReceived { .. } => "hook_received",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub session_id: String,
    pub event_type: EventType,
    pub payload: Option<serde_json::Value>,
    pub timestamp: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_session_discovered_serde() {
        let event_type = EventType::SessionDiscovered;
        let json = serde_json::to_string(&event_type).unwrap();
        assert!(json.contains("\"type\":\"session_discovered\""));

        let parsed: EventType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event_type);
    }

    #[test]
    fn test_event_type_session_removed_serde() {
        let event_type = EventType::SessionRemoved;
        let json = serde_json::to_string(&event_type).unwrap();
        assert!(json.contains("\"type\":\"session_removed\""));

        let parsed: EventType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event_type);
    }

    #[test]
    fn test_event_type_state_changed_serde() {
        let event_type = EventType::StateChanged {
            from: SessionState::Idle,
            to: SessionState::Working,
        };
        let json = serde_json::to_string(&event_type).unwrap();
        assert!(json.contains("\"type\":\"state_changed\""));
        assert!(json.contains("\"from\":\"idle\""));
        assert!(json.contains("\"to\":\"working\""));

        let parsed: EventType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event_type);
    }

    #[test]
    fn test_event_type_hook_received_serde() {
        let event_type = EventType::HookReceived {
            hook_type: "PostToolUse".to_string(),
        };
        let json = serde_json::to_string(&event_type).unwrap();
        assert!(json.contains("\"type\":\"hook_received\""));
        assert!(json.contains("\"hook_type\":\"PostToolUse\""));

        let parsed: EventType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event_type);
    }

    #[test]
    fn test_event_type_type_name() {
        assert_eq!(EventType::SessionDiscovered.type_name(), "session_discovered");
        assert_eq!(EventType::SessionRemoved.type_name(), "session_removed");
        assert_eq!(
            EventType::StateChanged {
                from: SessionState::Idle,
                to: SessionState::Done
            }
            .type_name(),
            "state_changed"
        );
        assert_eq!(
            EventType::HookReceived {
                hook_type: "test".to_string()
            }
            .type_name(),
            "hook_received"
        );
    }

    #[test]
    fn test_event_serialization_roundtrip() {
        let event = Event {
            id: 42,
            session_id: "test-session-id".to_string(),
            event_type: EventType::StateChanged {
                from: SessionState::Working,
                to: SessionState::NeedsInput,
            },
            payload: Some(serde_json::json!({"extra": "data"})),
            timestamp: 1706500000,
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn test_event_with_null_payload() {
        let event = Event {
            id: 1,
            session_id: "session-1".to_string(),
            event_type: EventType::SessionDiscovered,
            payload: None,
            timestamp: 1000,
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
        assert!(parsed.payload.is_none());
    }

    #[test]
    fn test_event_json_format() {
        let event = Event {
            id: 1,
            session_id: "sess-123".to_string(),
            event_type: EventType::SessionDiscovered,
            payload: None,
            timestamp: 1706500000,
        };

        let json = serde_json::to_string_pretty(&event).unwrap();
        assert!(json.contains("\"session_id\": \"sess-123\""));
        assert!(json.contains("\"timestamp\": 1706500000"));
    }
}
