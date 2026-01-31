use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Working,
    NeedsInput,
    Done,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Working => "working",
            SessionState::NeedsInput => "needs_input",
            SessionState::Done => "done",
        }
    }
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SessionState {
    type Err = ParseSessionStateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "idle" => Ok(SessionState::Idle),
            "working" => Ok(SessionState::Working),
            "needs_input" => Ok(SessionState::NeedsInput),
            "done" => Ok(SessionState::Done),
            _ => Err(ParseSessionStateError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSessionStateError(pub String);

impl fmt::Display for ParseSessionStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown session state: {}", self.0)
    }
}

impl std::error::Error for ParseSessionStateError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub pane_id: String,
    pub session_name: String,
    pub window_index: u32,
    pub pane_index: u32,
    pub working_dir: String,
    pub state: SessionState,
    pub detection_method: String,
    pub last_activity: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_state_as_str() {
        assert_eq!(SessionState::Idle.as_str(), "idle");
        assert_eq!(SessionState::Working.as_str(), "working");
        assert_eq!(SessionState::NeedsInput.as_str(), "needs_input");
        assert_eq!(SessionState::Done.as_str(), "done");
    }

    #[test]
    fn test_session_state_display() {
        assert_eq!(SessionState::Idle.to_string(), "idle");
        assert_eq!(SessionState::Working.to_string(), "working");
        assert_eq!(SessionState::NeedsInput.to_string(), "needs_input");
        assert_eq!(SessionState::Done.to_string(), "done");
    }

    #[test]
    fn test_session_state_from_str() {
        assert_eq!("idle".parse::<SessionState>(), Ok(SessionState::Idle));
        assert_eq!("working".parse::<SessionState>(), Ok(SessionState::Working));
        assert_eq!(
            "needs_input".parse::<SessionState>(),
            Ok(SessionState::NeedsInput)
        );
        assert_eq!("done".parse::<SessionState>(), Ok(SessionState::Done));
    }

    #[test]
    fn test_session_state_from_str_invalid() {
        let result = "unknown".parse::<SessionState>();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "unknown session state: unknown"
        );
    }

    #[test]
    fn test_session_state_serde_roundtrip() {
        for state in [
            SessionState::Idle,
            SessionState::Working,
            SessionState::NeedsInput,
            SessionState::Done,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let parsed: SessionState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, parsed);
        }
    }

    #[test]
    fn test_session_state_serde_matches_as_str() {
        assert_eq!(
            serde_json::to_string(&SessionState::Idle).unwrap(),
            "\"idle\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::Working).unwrap(),
            "\"working\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::NeedsInput).unwrap(),
            "\"needs_input\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::Done).unwrap(),
            "\"done\""
        );
    }

    #[test]
    fn test_session_serialization_roundtrip() {
        let session = Session {
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            pane_id: "%5".to_string(),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 1,
            working_dir: "/home/user/project".to_string(),
            state: SessionState::Working,
            detection_method: "process_name".to_string(),
            last_activity: 1706500000,
            created_at: 1706400000,
            updated_at: 1706500000,
        };

        let json = serde_json::to_string(&session).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(session, parsed);
    }

    #[test]
    fn test_session_json_format() {
        let session = Session {
            id: "test-id".to_string(),
            pane_id: "%0".to_string(),
            session_name: "dev".to_string(),
            window_index: 1,
            pane_index: 2,
            working_dir: "/tmp".to_string(),
            state: SessionState::Idle,
            detection_method: "pane_content".to_string(),
            last_activity: 100,
            created_at: 50,
            updated_at: 100,
        };

        let json = serde_json::to_string_pretty(&session).unwrap();
        assert!(json.contains("\"state\": \"idle\""));
        assert!(json.contains("\"pane_id\": \"%0\""));
    }
}
